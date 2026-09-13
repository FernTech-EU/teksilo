// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Mouse handling for the terminal view: mapping a pixel position to a grid
//! cell, and encoding mouse events as VT reports when the child program has
//! enabled mouse tracking. Pure, engine-independent logic (unit-tested here).

use crate::engine::TermMode;
use teksilo_core::event::Modifiers;

/// A pointer button as the terminal reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

/// What a **direct** pointer (a finger, a pen tip) is reported to the child
/// program as, while that program has mouse tracking enabled.
///
/// A mouse is not governed by this at all: it reports exactly what it has
/// always reported, gated only by the `Shift` override.
///
/// The default is [`Off`](Self::Off), which is the decision the touch
/// programme took for the shipped terminal, and it is a decision rather than a
/// simplification. A finger has no hover, no buttons and no sub-cell
/// resolution, so reporting it as a mouse gives a full-screen program a stream
/// it cannot use well, while spending the only gesture vocabulary the *view*
/// has: with the contact routed to the child there is no tap to select with,
/// no hold to open a menu with, and no drag left to scroll the scrollback
/// with. Terminals that do offer it (Termux, Blink) put it behind a setting,
/// which is what this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TouchReporting {
    /// A direct pointer is never reported. It selects, scrolls and opens the
    /// context menu locally, whatever mode the child has set.
    #[default]
    Off,
    /// A single direct contact is reported as **mouse button 1** — the same
    /// bytes a left mouse button produces, because that is what a child
    /// program understands: `ESC [ < 0 ; col ; row M` on press, then `32`
    /// (button 1 plus the motion bit) on each move, then `... m` on release
    /// under SGR — and the legacy X10 triple otherwise. Nothing distinguishes it
    /// from a mouse on the wire; there is no VT encoding for "this was a
    /// finger".
    ///
    /// Two or more simultaneous contacts are **never** reported: a two-finger
    /// pan stays local and scrolls the scrollback, which is the only way back
    /// to the scrollback once one finger belongs to the child.
    AsButton1,
}

/// The kind of pointer event being reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Press,
    Release,
    /// Motion with a button held (mode 1002).
    Drag,
    /// Motion with no button held (mode 1003).
    Motion,
}

/// Map a pixel position (relative to the grid's content origin) to a `(col,
/// row)` cell, clamped to the grid bounds.
pub fn cell_at(
    x: f32,
    y: f32,
    cell_w: f32,
    cell_h: f32,
    cols: usize,
    rows: usize,
) -> (usize, usize) {
    let col = if cell_w > 0.0 {
        (x / cell_w).floor().max(0.0) as usize
    } else {
        0
    };
    let row = if cell_h > 0.0 {
        (y / cell_h).floor().max(0.0) as usize
    } else {
        0
    };
    (
        col.min(cols.saturating_sub(1)),
        row.min(rows.saturating_sub(1)),
    )
}

/// Encode a mouse event as a VT report, or `None` when the current
/// [`TermMode`] does not request reporting for this event. `col`/`row` are
/// 0-based cell coordinates.
pub fn encode_mouse(
    kind: MouseKind,
    button: MouseButton,
    col: usize,
    row: usize,
    mods: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !mode.mouse_reporting() {
        return None;
    }
    let allowed = match kind {
        MouseKind::Press | MouseKind::Release => true,
        MouseKind::Drag => mode.mouse_drag || mode.mouse_motion,
        MouseKind::Motion => mode.mouse_motion,
    };
    if !allowed {
        return None;
    }

    let mut cb: u32 = match kind {
        // Motion with no button held is reported as "button 3 (none) + motion".
        MouseKind::Motion => 3 + 32,
        _ => {
            let base = match button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                MouseButton::WheelUp => 64,
                MouseButton::WheelDown => 65,
            };
            if kind == MouseKind::Drag {
                base + 32
            } else {
                base
            }
        }
    };
    if mods.shift() {
        cb += 4;
    }
    if mods.alt() {
        cb += 8;
    }
    if mods.ctrl() {
        cb += 16;
    }

    let x = col + 1;
    let y = row + 1;

    if mode.sgr_mouse {
        let final_byte = if kind == MouseKind::Release {
            b'm'
        } else {
            b'M'
        };
        let mut out = Vec::with_capacity(12);
        out.extend_from_slice(b"\x1b[<");
        out.extend_from_slice(format!("{cb};{x};{y}").as_bytes());
        out.push(final_byte);
        Some(out)
    } else {
        // Legacy X10 encoding: one byte each for button/x/y, all offset by 32.
        // A release is encoded as button 3 (keeping the modifier/motion bits).
        let cb = if kind == MouseKind::Release {
            (cb & !0b11) | 0b11
        } else {
            cb
        };
        // The legacy form can't encode coordinates past column/row 223.
        if x > 223 || y > 223 {
            return None;
        }
        Some(vec![
            0x1b,
            b'[',
            b'M',
            (cb + 32) as u8,
            (x + 32) as u8,
            (y + 32) as u8,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracking(sgr: bool) -> TermMode {
        TermMode {
            mouse_report_click: true,
            mouse_drag: true,
            mouse_motion: true,
            sgr_mouse: sgr,
            ..TermMode::default()
        }
    }

    #[test]
    fn no_report_without_tracking() {
        assert_eq!(
            encode_mouse(
                MouseKind::Press,
                MouseButton::Left,
                0,
                0,
                Modifiers::NONE,
                TermMode::default()
            ),
            None
        );
    }

    #[test]
    fn sgr_press_release() {
        // Left press at (col 4, row 2) → ESC [ < 0 ; 5 ; 3 M
        assert_eq!(
            encode_mouse(
                MouseKind::Press,
                MouseButton::Left,
                4,
                2,
                Modifiers::NONE,
                tracking(true)
            ),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        // Left release → same coords, 'm'
        assert_eq!(
            encode_mouse(
                MouseKind::Release,
                MouseButton::Left,
                4,
                2,
                Modifiers::NONE,
                tracking(true)
            ),
            Some(b"\x1b[<0;5;3m".to_vec())
        );
    }

    #[test]
    fn sgr_wheel_and_modifiers() {
        // Wheel up (64) with Ctrl (+16) = 80 at (0,0) → ESC [ < 80 ; 1 ; 1 M
        assert_eq!(
            encode_mouse(
                MouseKind::Press,
                MouseButton::WheelUp,
                0,
                0,
                Modifiers::CTRL,
                tracking(true)
            ),
            Some(b"\x1b[<80;1;1M".to_vec())
        );
    }

    #[test]
    fn drag_adds_motion_bit() {
        // Left drag = 0 + 32 = 32
        assert_eq!(
            encode_mouse(
                MouseKind::Drag,
                MouseButton::Left,
                0,
                0,
                Modifiers::NONE,
                tracking(true)
            ),
            Some(b"\x1b[<32;1;1M".to_vec())
        );
    }

    #[test]
    fn motion_only_needs_any_motion_mode() {
        let drag_only = TermMode {
            mouse_report_click: true,
            mouse_drag: true,
            ..TermMode::default()
        };
        // Pure motion is not reported in drag-only (1002) mode.
        assert_eq!(
            encode_mouse(
                MouseKind::Motion,
                MouseButton::Left,
                0,
                0,
                Modifiers::NONE,
                drag_only
            ),
            None
        );
        // But is in any-motion (1003) mode: button 3 + motion 32 = 35.
        assert_eq!(
            encode_mouse(
                MouseKind::Motion,
                MouseButton::Left,
                0,
                0,
                Modifiers::NONE,
                tracking(false)
            ),
            Some(vec![0x1b, b'[', b'M', (35 + 32) as u8, 33, 33])
        );
    }

    #[test]
    fn legacy_x10_press() {
        // Left press at (0,0), legacy: button 32, x 33, y 33.
        assert_eq!(
            encode_mouse(
                MouseKind::Press,
                MouseButton::Left,
                0,
                0,
                Modifiers::NONE,
                tracking(false)
            ),
            Some(vec![0x1b, b'[', b'M', 32, 33, 33])
        );
    }

    #[test]
    fn cell_at_clamps() {
        assert_eq!(cell_at(0.0, 0.0, 8.0, 16.0, 80, 24), (0, 0));
        assert_eq!(cell_at(20.0, 33.0, 8.0, 16.0, 80, 24), (2, 2));
        // Beyond the grid clamps to the last cell.
        assert_eq!(cell_at(10_000.0, 10_000.0, 8.0, 16.0, 80, 24), (79, 23));
    }
}
