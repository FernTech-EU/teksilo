// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

/// Cursor icon for the mouse pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorIcon {
    Default,
    Pointer,
    Text,
    Crosshair,
    Move,
    NotAllowed,
    Grab,
    Grabbing,
    ColResize,
    RowResize,
    /// NE/SW diagonal resize — used on the top-right and bottom-left
    /// corners of a resize frame. Maps to winit's `NeswResize`.
    NeswResize,
    /// NW/SE diagonal resize — used on the top-left and bottom-right
    /// corners of a resize frame. Maps to winit's `NwseResize`.
    NwseResize,
}
