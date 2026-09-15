// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `POINTER_PEN_INFO` / `POINTER_TOUCH_INFO`, decoded from bytes.
//!
//! This half of the Windows shim is **target-independent on purpose**. The
//! Win32 side of it is three calls (`GetPointerType`, `GetPointerPenInfo`,
//! `GetPointerTouchInfo`) that fill a `#[repr(C)]` struct; everything
//! interesting afterwards — the normalisations, the flag→tool folding, the
//! himetric-free coordinate handling — is arithmetic on that struct's bytes.
//! Keeping it here, over a `&[u8]`, means the whole of it is exercised from a
//! Linux CI runner against recorded layouts, and the live path on Windows runs
//! **the same code** over the bytes the OS just wrote.
//!
//! # Provenance of the layout
//!
//! Field order is Microsoft's (`winuser.h` / the `POINTER_PEN_INFO`,
//! `POINTER_TOUCH_INFO` and `POINTER_INFO` documentation); the offsets are that
//! order laid out by the **64-bit** Windows C ABI (4-byte scalars, 8-byte
//! `HANDLE` / `HWND` / `UINT64`, struct alignment 8). They are *derived*, not
//! captured from a machine — but they are not taken on trust either: compiled
//! for Windows, this module asserts every one of them against `windows-rs`'s
//! own `POINTER_PEN_INFO` with [`core::mem::offset_of`], so a wrong constant is
//! a build failure on the platform that matters rather than a silent misread.
//!
//! A 32-bit Windows target has 4-byte pointers and would need its own table;
//! the assertions below will refuse to compile there rather than decode
//! garbage.
//!
//! # The coalescing history
//!
//! A `WM_POINTERUPDATE` is not one digitizer packet. Windows merges the
//! packets that arrived between two messages and tells you how many in
//! `POINTER_INFO::historyCount`; `GetPointerPenInfo` hands back only the
//! newest of them. Reading just that caps a 200-360 Hz stylus at the window
//! message rate, which is roughly the display's.
//!
//! [`decode_pen_history`] is the other half: it parses the array
//! `GetPointerPenInfoHistory` fills and **reverses** it, because Win32 orders
//! that array newest-first and `PenSource::poll` promises oldest-first. That
//! reversal is the single most reversible mistake in this file — get it wrong
//! and every stroke is drawn backwards — so it is stated at the function, and
//! pinned by a test that decodes a synthesised three-entry history.
//!
//! Reference: `docs/touch-and-pen.md`, "Pen and stylus".

use teksilo_canvas::{Point, Size};
use teksilo_tokens::PenKind;

use crate::pen::{PenButtons, PenPacket};

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Byte offsets within `POINTER_INFO`, and the sizes of the three structs.
///
/// Public so a porter can read the table without reading the parser.
pub mod layout {
    /// `POINTER_INFO::pointerType` (`POINTER_INPUT_TYPE`, a 32-bit enum).
    pub const POINTER_TYPE: usize = 0;
    /// `POINTER_INFO::pointerId`.
    pub const POINTER_ID: usize = 4;
    /// `POINTER_INFO::pointerFlags`.
    pub const POINTER_FLAGS: usize = 12;
    /// `POINTER_INFO::ptPixelLocation.x` — **screen** physical pixels.
    pub const PIXEL_X: usize = 32;
    /// `POINTER_INFO::ptPixelLocation.y`.
    pub const PIXEL_Y: usize = 36;
    /// `POINTER_INFO::dwTime`, a `GetTickCount`-based millisecond stamp.
    pub const TIME: usize = 64;
    /// `POINTER_INFO::historyCount` — how many digitizer packets the OS
    /// coalesced into this one message. `1` when nothing was coalesced.
    pub const HISTORY_COUNT: usize = 68;
    /// `size_of::<POINTER_INFO>()`.
    pub const POINTER_INFO_SIZE: usize = 96;

    /// `POINTER_PEN_INFO::penFlags`.
    pub const PEN_FLAGS: usize = 96;
    /// `POINTER_PEN_INFO::penMask`.
    pub const PEN_MASK: usize = 100;
    /// `POINTER_PEN_INFO::pressure`, `0..=1024`.
    pub const PEN_PRESSURE: usize = 104;
    /// `POINTER_PEN_INFO::rotation`, `0..=359` degrees.
    pub const PEN_ROTATION: usize = 108;
    /// `POINTER_PEN_INFO::tiltX`, `-90..=90` degrees.
    pub const PEN_TILT_X: usize = 112;
    /// `POINTER_PEN_INFO::tiltY`, `-90..=90` degrees.
    pub const PEN_TILT_Y: usize = 116;
    /// `size_of::<POINTER_PEN_INFO>()`.
    pub const PEN_INFO_SIZE: usize = 120;

    /// `POINTER_TOUCH_INFO::touchMask`.
    pub const TOUCH_MASK: usize = 100;
    /// `POINTER_TOUCH_INFO::rcContact.left` — screen physical pixels.
    pub const TOUCH_CONTACT_LEFT: usize = 104;
    /// `POINTER_TOUCH_INFO::rcContact.top`.
    pub const TOUCH_CONTACT_TOP: usize = 108;
    /// `POINTER_TOUCH_INFO::rcContact.right`.
    pub const TOUCH_CONTACT_RIGHT: usize = 112;
    /// `POINTER_TOUCH_INFO::rcContact.bottom`.
    pub const TOUCH_CONTACT_BOTTOM: usize = 116;
    /// `POINTER_TOUCH_INFO::orientation`, `0..=359` degrees.
    pub const TOUCH_ORIENTATION: usize = 136;
    /// `POINTER_TOUCH_INFO::pressure`, `0..=1024`.
    pub const TOUCH_PRESSURE: usize = 140;
    /// `size_of::<POINTER_TOUCH_INFO>()`.
    pub const TOUCH_INFO_SIZE: usize = 144;
}

// The one place the derived table meets the real ABI. `windows-rs` generates
// these structs from Microsoft's own metadata, so an offset that disagrees is
// our bug, caught at compile time on the only platform that can run this code.
#[cfg(target_os = "windows")]
mod abi_assertions {
    use super::layout::*;
    use core::mem::{offset_of, size_of};
    use windows::Win32::UI::Input::Pointer::{POINTER_INFO, POINTER_PEN_INFO, POINTER_TOUCH_INFO};

    const _: () = assert!(size_of::<POINTER_INFO>() == POINTER_INFO_SIZE);
    const _: () = assert!(size_of::<POINTER_PEN_INFO>() == PEN_INFO_SIZE);
    const _: () = assert!(size_of::<POINTER_TOUCH_INFO>() == TOUCH_INFO_SIZE);

    const _: () = assert!(offset_of!(POINTER_INFO, pointerType) == POINTER_TYPE);
    const _: () = assert!(offset_of!(POINTER_INFO, pointerId) == POINTER_ID);
    const _: () = assert!(offset_of!(POINTER_INFO, pointerFlags) == POINTER_FLAGS);
    const _: () = assert!(offset_of!(POINTER_INFO, ptPixelLocation) == PIXEL_X);
    const _: () = assert!(offset_of!(POINTER_INFO, dwTime) == TIME);
    const _: () = assert!(offset_of!(POINTER_INFO, historyCount) == HISTORY_COUNT);

    const _: () = assert!(offset_of!(POINTER_PEN_INFO, penFlags) == PEN_FLAGS);
    const _: () = assert!(offset_of!(POINTER_PEN_INFO, penMask) == PEN_MASK);
    const _: () = assert!(offset_of!(POINTER_PEN_INFO, pressure) == PEN_PRESSURE);
    const _: () = assert!(offset_of!(POINTER_PEN_INFO, rotation) == PEN_ROTATION);
    const _: () = assert!(offset_of!(POINTER_PEN_INFO, tiltX) == PEN_TILT_X);
    const _: () = assert!(offset_of!(POINTER_PEN_INFO, tiltY) == PEN_TILT_Y);

    const _: () = assert!(offset_of!(POINTER_TOUCH_INFO, touchMask) == TOUCH_MASK);
    const _: () = assert!(offset_of!(POINTER_TOUCH_INFO, rcContact) == TOUCH_CONTACT_LEFT);
    const _: () = assert!(offset_of!(POINTER_TOUCH_INFO, orientation) == TOUCH_ORIENTATION);
    const _: () = assert!(offset_of!(POINTER_TOUCH_INFO, pressure) == TOUCH_PRESSURE);
}

// ---------------------------------------------------------------------------
// Constants (values from winuser.h; `windows-rs` exposes some of them, but the
// decoder must build off Windows too, so they are named here.)
// ---------------------------------------------------------------------------

/// `PT_POINTER` — a generic pointer of unknown provenance.
pub const PT_POINTER: u32 = 1;
/// `PT_TOUCH`.
pub const PT_TOUCH: u32 = 2;
/// `PT_PEN`.
pub const PT_PEN: u32 = 3;
/// `PT_MOUSE`.
pub const PT_MOUSE: u32 = 4;
/// `PT_TOUCHPAD`.
pub const PT_TOUCHPAD: u32 = 5;

/// `POINTER_FLAG_INRANGE` — the tool is within detection range (proximity).
pub const POINTER_FLAG_INRANGE: u32 = 0x0000_0002;
/// `POINTER_FLAG_INCONTACT` — the tip is touching the surface.
pub const POINTER_FLAG_INCONTACT: u32 = 0x0000_0004;
/// `POINTER_FLAG_FIRSTBUTTON` — the tip (or left mouse button).
pub const POINTER_FLAG_FIRSTBUTTON: u32 = 0x0000_0010;
/// `POINTER_FLAG_SECONDBUTTON` — the barrel button.
pub const POINTER_FLAG_SECONDBUTTON: u32 = 0x0000_0020;
/// `POINTER_FLAG_THIRDBUTTON` — a second barrel button, where present.
pub const POINTER_FLAG_THIRDBUTTON: u32 = 0x0000_0040;
/// `POINTER_FLAG_PRIMARY` — this pointer drives the legacy singular signals.
pub const POINTER_FLAG_PRIMARY: u32 = 0x0000_2000;
/// `POINTER_FLAG_CANCELED` — the input was cancelled (a palm, a lost capture).
pub const POINTER_FLAG_CANCELED: u32 = 0x0000_8000;

/// `PEN_FLAG_BARREL` — the barrel button is pressed.
pub const PEN_FLAG_BARREL: u32 = 0x0000_0001;
/// `PEN_FLAG_INVERTED` — the stylus is held eraser-end down.
pub const PEN_FLAG_INVERTED: u32 = 0x0000_0002;
/// `PEN_FLAG_ERASER` — the eraser button is pressed.
pub const PEN_FLAG_ERASER: u32 = 0x0000_0004;

/// `PEN_MASK_PRESSURE` — `pressure` is valid.
pub const PEN_MASK_PRESSURE: u32 = 0x0000_0001;
/// `PEN_MASK_ROTATION` — `rotation` is valid.
pub const PEN_MASK_ROTATION: u32 = 0x0000_0002;
/// `PEN_MASK_TILT_X` — `tiltX` is valid.
pub const PEN_MASK_TILT_X: u32 = 0x0000_0004;
/// `PEN_MASK_TILT_Y` — `tiltY` is valid.
pub const PEN_MASK_TILT_Y: u32 = 0x0000_0008;

/// `TOUCH_MASK_CONTACTAREA` — `rcContact` is valid.
pub const TOUCH_MASK_CONTACTAREA: u32 = 0x0000_0001;
/// `TOUCH_MASK_ORIENTATION` — `orientation` is valid.
pub const TOUCH_MASK_ORIENTATION: u32 = 0x0000_0002;
/// `TOUCH_MASK_PRESSURE` — `pressure` is valid.
pub const TOUCH_MASK_PRESSURE: u32 = 0x0000_0004;

/// The digitizer's pressure range: `0..=1024`, so 512 is exactly half.
pub const PRESSURE_RANGE: f32 = 1024.0;

// ---------------------------------------------------------------------------
// Decoded forms
// ---------------------------------------------------------------------------

/// A decoded `POINTER_PEN_INFO`, still in the OS's units and coordinate space.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct RawPenInfo {
    /// `POINTER_INPUT_TYPE` — one of the `PT_*` constants.
    pub pointer_type: u32,
    /// The OS contact id.
    pub pointer_id: u32,
    /// `POINTER_FLAGS`.
    pub flags: u32,
    /// `PEN_FLAGS`.
    pub pen_flags: u32,
    /// `PEN_MASK` — which of the axes below carry a real measurement.
    pub pen_mask: u32,
    /// Raw pressure, `0..=1024`.
    pub pressure: u32,
    /// Raw rotation, `0..=359` degrees.
    pub rotation: u32,
    /// Raw tilt, `-90..=90` degrees.
    pub tilt_x: i32,
    /// Raw tilt, `-90..=90` degrees.
    pub tilt_y: i32,
    /// `ptPixelLocation`, in **screen** physical pixels.
    pub screen: (i32, i32),
    /// `dwTime`, milliseconds on the OS's tick clock.
    pub time_ms: u32,
    /// `historyCount` — how many digitizer packets the OS coalesced into the
    /// message this struct came from.
    ///
    /// `1` when nothing was coalesced (and `0` from a driver that does not
    /// fill the field at all); anything above that is the number of entries
    /// [`GetPointerPenInfoHistory`] has waiting, each with its own `dwTime`,
    /// its own pressure and its own tilt. Reading only the newest — which is
    /// what `GetPointerPenInfo` alone gives you — throws the rest away and
    /// caps a stylus at the window message rate.
    ///
    /// [`GetPointerPenInfoHistory`]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getpointerpeninfohistory
    pub history_count: u32,
}

impl RawPenInfo {
    /// Whether the tool is within detection range.
    pub const fn in_proximity(&self) -> bool {
        self.flags & POINTER_FLAG_INRANGE != 0
    }

    /// Whether the tip is touching the surface.
    pub const fn down(&self) -> bool {
        self.flags & POINTER_FLAG_INCONTACT != 0
    }

    /// Whether the OS says this input was cancelled (a palm, a lost capture).
    pub const fn cancelled(&self) -> bool {
        self.flags & POINTER_FLAG_CANCELED != 0
    }

    /// The tool the digitizer is reporting.
    ///
    /// `PEN_FLAG_INVERTED` (the stylus is flipped) and `PEN_FLAG_ERASER` (the
    /// eraser *button* is held on a stylus that has one) both mean the user is
    /// erasing, and Teksilo represents erasing as a tool rather than a button —
    /// so both fold to [`PenKind::Eraser`]. Windows Ink reports no finer tool
    /// taxonomy through this struct: brush, pencil and airbrush are Wacom
    /// driver concepts that arrive as an ordinary pen here, which is why they
    /// are absent rather than guessed at.
    pub const fn tool(&self) -> PenKind {
        if self.pen_flags & (PEN_FLAG_INVERTED | PEN_FLAG_ERASER) != 0 {
            PenKind::Eraser
        } else {
            PenKind::Pen
        }
    }

    /// Normalised tip pressure, or `None` when `PEN_MASK_PRESSURE` is clear.
    ///
    /// `0 → 0.0`, `512 → 0.5`, `1024 → 1.0`. Values above the documented range
    /// are clamped rather than trusted.
    pub fn pressure_normalised(&self) -> Option<f32> {
        if self.pen_mask & PEN_MASK_PRESSURE == 0 {
            return None;
        }
        Some((self.pressure as f32 / PRESSURE_RANGE).clamp(0.0, 1.0))
    }

    /// Tilt in degrees, or `None` unless **both** axes are marked valid.
    ///
    /// Both or neither: a `PointerAxes::tilt` is a pair, and half a pair is
    /// worse than none — a consumer computing a brush angle from a real
    /// `tilt_x` and a zeroed `tilt_y` would draw a confidently wrong stroke.
    pub fn tilt_degrees(&self) -> Option<(f32, f32)> {
        if self.pen_mask & (PEN_MASK_TILT_X | PEN_MASK_TILT_Y)
            != (PEN_MASK_TILT_X | PEN_MASK_TILT_Y)
        {
            return None;
        }
        Some((
            (self.tilt_x as f32).clamp(-90.0, 90.0),
            (self.tilt_y as f32).clamp(-90.0, 90.0),
        ))
    }

    /// Barrel rotation in degrees, or `None` when `PEN_MASK_ROTATION` is clear.
    pub fn twist_degrees(&self) -> Option<f32> {
        if self.pen_mask & PEN_MASK_ROTATION == 0 {
            return None;
        }
        Some((self.rotation as f32).clamp(0.0, 359.0))
    }

    /// The stylus buttons held.
    ///
    /// Read from **both** places the OS reports them: `PEN_FLAG_BARREL` and the
    /// generic `POINTER_FLAG_SECONDBUTTON`. Drivers differ on which they set,
    /// and a barrel press that reached only one of the two would be a button
    /// that works on some tablets and not others.
    pub const fn buttons(&self) -> PenButtons {
        PenButtons::NONE
            .with(
                PenButtons::BARREL,
                self.pen_flags & PEN_FLAG_BARREL != 0
                    || self.flags & POINTER_FLAG_SECONDBUTTON != 0,
            )
            .with(
                PenButtons::SECONDARY_BARREL,
                self.flags & POINTER_FLAG_THIRDBUTTON != 0,
            )
            .with(PenButtons::ERASER, self.pen_flags & PEN_FLAG_ERASER != 0)
    }

    /// Turn this into a [`PenPacket`] in window-logical coordinates.
    ///
    /// `client_origin` is the window's client-area origin in screen physical
    /// pixels (what `ClientToScreen(hwnd, {0,0})` answers) and `scale` is the
    /// window's DPI scale factor, so the arithmetic here is the same
    /// physical→logical division the rest of the translator does.
    ///
    /// Pressure that the device did not report becomes `1.0` while the tip is
    /// down and `0.0` while it hovers: a tip in contact has *some* pressure,
    /// and reporting `0.0` would make a pressure-driven brush paint nothing on
    /// hardware that simply has no sensor.
    pub fn to_packet(&self, client_origin: (i32, i32), scale: f32) -> PenPacket {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let position = Point::new(
            (self.screen.0 - client_origin.0) as f32 / scale,
            (self.screen.1 - client_origin.1) as f32 / scale,
        );
        let down = self.down();
        PenPacket {
            tool: self.tool(),
            position,
            pressure: self
                .pressure_normalised()
                .unwrap_or(if down { 1.0 } else { 0.0 }),
            tilt: self.tilt_degrees(),
            twist: self.twist_degrees(),
            buttons: self.buttons(),
            in_proximity: self.in_proximity(),
            down,
            // `dwTime` is a `GetTickCount` stamp with no known offset from the
            // tree's epoch, so it is carried as the raw counter it is and read
            // only as a difference within one drained batch. See
            // `PenPacket::device_time_ms`.
            device_time_ms: Some(self.time_ms),
        }
    }
}

/// A decoded `POINTER_TOUCH_INFO` — the fields `WM_TOUCH` cannot carry.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct RawTouchInfo {
    /// The OS contact id — the value winit puts in `Touch::id` on the
    /// `WM_POINTER` path.
    pub pointer_id: u32,
    /// `POINTER_FLAGS`.
    pub flags: u32,
    /// `TOUCH_MASK`.
    pub touch_mask: u32,
    /// `rcContact` as `(left, top, right, bottom)` in screen physical pixels.
    pub contact: (i32, i32, i32, i32),
    /// Contact orientation in degrees.
    pub orientation: u32,
    /// Raw pressure, `0..=1024`.
    pub pressure: u32,
}

impl RawTouchInfo {
    /// The contact patch in **logical** pixels, or `None` when
    /// `TOUCH_MASK_CONTACTAREA` is clear.
    ///
    /// This is the datum the whole touch arm of this shim exists for:
    /// `WM_TOUCH`, the path winit 0.30 takes, has no contact geometry at all,
    /// and both the palm heuristic and finger-avoiding overlay placement need
    /// it.
    pub fn contact_size(&self, scale: f32) -> Option<Size> {
        if self.touch_mask & TOUCH_MASK_CONTACTAREA == 0 {
            return None;
        }
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let (left, top, right, bottom) = self.contact;
        // A degenerate rectangle is not a measurement.
        if right <= left || bottom <= top {
            return None;
        }
        Some(Size::new(
            (right - left) as f32 / scale,
            (bottom - top) as f32 / scale,
        ))
    }

    /// Normalised pressure, or `None` when `TOUCH_MASK_PRESSURE` is clear.
    pub fn pressure_normalised(&self) -> Option<f32> {
        if self.touch_mask & TOUCH_MASK_PRESSURE == 0 {
            return None;
        }
        Some((self.pressure as f32 / PRESSURE_RANGE).clamp(0.0, 1.0))
    }
}

// ---------------------------------------------------------------------------
// The parsers
// ---------------------------------------------------------------------------

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

fn i32_at(bytes: &[u8], offset: usize) -> Option<i32> {
    u32_at(bytes, offset).map(|v| v as i32)
}

/// Decode a `POINTER_PEN_INFO`. `None` if the slice is short.
pub fn decode_pen_info(bytes: &[u8]) -> Option<RawPenInfo> {
    if bytes.len() < layout::PEN_INFO_SIZE {
        return None;
    }
    Some(RawPenInfo {
        pointer_type: u32_at(bytes, layout::POINTER_TYPE)?,
        pointer_id: u32_at(bytes, layout::POINTER_ID)?,
        flags: u32_at(bytes, layout::POINTER_FLAGS)?,
        pen_flags: u32_at(bytes, layout::PEN_FLAGS)?,
        pen_mask: u32_at(bytes, layout::PEN_MASK)?,
        pressure: u32_at(bytes, layout::PEN_PRESSURE)?,
        rotation: u32_at(bytes, layout::PEN_ROTATION)?,
        tilt_x: i32_at(bytes, layout::PEN_TILT_X)?,
        tilt_y: i32_at(bytes, layout::PEN_TILT_Y)?,
        screen: (
            i32_at(bytes, layout::PIXEL_X)?,
            i32_at(bytes, layout::PIXEL_Y)?,
        ),
        time_ms: u32_at(bytes, layout::TIME)?,
        history_count: u32_at(bytes, layout::HISTORY_COUNT)?,
    })
}

// ---------------------------------------------------------------------------
// The coalescing history
// ---------------------------------------------------------------------------

/// The most history entries this shim will ever ask the OS for.
///
/// `POINTER_INFO::historyCount` is a number a driver writes, and a buffer
/// sized from an untrusted count is a buffer waiting to be told to be a
/// gigabyte. 512 is far past anything a digitizer produces between two window
/// messages — a 360 Hz tablet fills about six entries per 16 ms frame — so the
/// cap costs nothing real and bounds the allocation absolutely.
pub const MAX_PEN_HISTORY_ENTRIES: usize = 512;

/// How many `POINTER_PEN_INFO` entries to request for a message whose
/// `POINTER_INFO::historyCount` is `history_count`.
///
/// `None` means "do not call the history API at all": a count of `0` or `1` is
/// a message that coalesced nothing (and `0` is also what a driver that never
/// fills the field leaves behind), so the single `GetPointerPenInfo` the shim
/// already made is the whole story and a second syscall would buy one
/// duplicate packet.
pub const fn history_entries_to_request(history_count: u32) -> Option<usize> {
    if history_count <= 1 {
        return None;
    }
    let wanted = history_count as usize;
    Some(if wanted > MAX_PEN_HISTORY_ENTRIES {
        MAX_PEN_HISTORY_ENTRIES
    } else {
        wanted
    })
}

/// Decode a `GetPointerPenInfoHistory` buffer into **oldest-first** packets.
///
/// # Ordering — the thing that silently reverses a stroke
///
/// Win32 fills the array in **reverse chronological order**: index 0 is the
/// **newest** entry, and the oldest is at `entries - 1`. (`GetPointerInfoHistory`
/// and its per-type siblings all document this the same way, and
/// `GetPointerPenInfoHistory` inherits it.)
///
/// [`PenSource::poll`](crate::pen::PenSource::poll) promises the opposite —
/// oldest first, because the caller reads a drained batch as one forward
/// timeline and back-dates it from the last entry. **This function is where
/// the two meet, and it reverses.** Getting it wrong does not fail: it draws
/// every stroke backwards, with the pressure ramp inverted and the velocity
/// pointing the wrong way.
///
/// `entries` is the count the OS wrote back through `entriesCount`, not the
/// count that was asked for. Entries past the end of `bytes`, and any entry
/// whose fixed-size image is short, are dropped rather than guessed at.
pub fn decode_pen_history(bytes: &[u8], entries: usize) -> Vec<RawPenInfo> {
    let available = bytes.len() / layout::PEN_INFO_SIZE;
    let entries = entries.min(available);
    let mut decoded = Vec::with_capacity(entries);
    for index in 0..entries {
        let start = index * layout::PEN_INFO_SIZE;
        let Some(info) = decode_pen_info(&bytes[start..start + layout::PEN_INFO_SIZE]) else {
            break;
        };
        decoded.push(info);
    }
    // Newest-first in, oldest-first out. See the ordering note above.
    decoded.reverse();
    decoded
}

/// Decode a `POINTER_TOUCH_INFO`. `None` if the slice is short.
pub fn decode_touch_info(bytes: &[u8]) -> Option<RawTouchInfo> {
    if bytes.len() < layout::TOUCH_INFO_SIZE {
        return None;
    }
    Some(RawTouchInfo {
        pointer_id: u32_at(bytes, layout::POINTER_ID)?,
        flags: u32_at(bytes, layout::POINTER_FLAGS)?,
        touch_mask: u32_at(bytes, layout::TOUCH_MASK)?,
        contact: (
            i32_at(bytes, layout::TOUCH_CONTACT_LEFT)?,
            i32_at(bytes, layout::TOUCH_CONTACT_TOP)?,
            i32_at(bytes, layout::TOUCH_CONTACT_RIGHT)?,
            i32_at(bytes, layout::TOUCH_CONTACT_BOTTOM)?,
        ),
        orientation: u32_at(bytes, layout::TOUCH_ORIENTATION)?,
        pressure: u32_at(bytes, layout::TOUCH_PRESSURE)?,
    })
}

#[cfg(test)]
mod tests;
