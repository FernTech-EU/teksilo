// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The pen source for a platform that has none — X11 and macOS.
//!
//! Both could grow one. X11 has XInput2 valuators, which is how GIMP and Krita
//! read a tablet; macOS has `NSEvent`'s `tabletPoint` / `tabletProximity`
//! subtypes. Neither is implemented here, for the same reason: winit 0.31 will
//! supply both through its own `TabletTool*` events, and a shim written now
//! would be deleted before it earned its maintenance. The two shims that *are*
//! written cover the platforms where a stylus is common and where the pen data
//! is already flowing past the app unread — a Windows Ink tablet and a Wayland
//! session with a Wacom.
//!
//! The point of this type is that the absence is **declared** rather than
//! faked: [`PenCaps::NONE`] flows into the window's `BackendCaps`, so a
//! consumer asking "does this window report tilt?" gets `false` instead of
//! reading a zero that could have been a real measurement.

use super::{PenCaps, PenPacket, PenSource};

/// A pen source that never has a pen.
#[derive(Copy, Clone, Debug, Default)]
pub struct NullPenSource;

impl NullPenSource {
    /// The one and only value.
    pub const fn new() -> Self {
        Self
    }
}

impl PenSource for NullPenSource {
    fn poll(&mut self, _out: &mut Vec<PenPacket>) {}

    fn capabilities(&self) -> PenCaps {
        PenCaps::NONE
    }
}
