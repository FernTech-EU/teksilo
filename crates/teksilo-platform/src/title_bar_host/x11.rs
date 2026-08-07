// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! X11 title bar host.
//!
//! Like the Wayland backend this is pure delegation to winit — no raw X11 code
//! is needed for the move/resize operations themselves. winit's X11
//! `drag_window` / `drag_resize_window` already implement the EWMH
//! `_NET_WM_MOVERESIZE` handshake correctly, including the mandatory
//! `XUngrabPointer` before the client message (skipping that is what makes a
//! window jump when the drag starts, because the WM cannot take the pointer
//! while the client still holds the implicit grab from the button press).
//!
//! Two things differ from Wayland:
//!
//! - **The host refuses to exist without a capable window manager.** Server-side
//!   decorations are suppressed via `_MOTIF_WM_HINTS`, and `_NET_WM_MOVERESIZE`
//!   is then the *only* way the window can be moved or resized. If the WM does
//!   not advertise it, [`X11Host::new`] returns [`PlatformError::Unsupported`],
//!   the factory hands back `None`, and the app keeps native decorations —
//!   rather than shipping a borderless window the user cannot move.
//!   [`crate::x11::ewmh`] runs the probe once per process, before window
//!   creation, because the decoration flag has to be decided at
//!   `WindowAttributes` time.
//! - **There is no system window menu.** winit's X11 `show_window_menu` is an
//!   empty stub, and `_GTK_SHOW_WINDOW_MENU` is unimplemented by KWin
//!   (KDE bug 454756), so `has_window_menu` reports `false` and the `TitleBar`
//!   widget builds its own.
//!
//! Keyboard move/resize is *not* lost by going borderless: Alt+F7 / Alt+F8 (and
//! the WM's own window-menu shortcut) are global window-manager bindings that
//! work regardless of who draws the frame.

use std::sync::Arc;

use teksilo_canvas::{Point, Size};
use teksilo_core::{
    HitRegions, PlatformError, PlatformTitleBarHost, ResizeEdge, TitleBarHostCallbacks,
};
use winit::window::Window;

use super::edge_to_direction;

pub struct X11Host {
    window: Arc<Window>,
}

impl X11Host {
    pub fn new(
        window: Arc<Window>,
        _callbacks: TitleBarHostCallbacks,
    ) -> Result<Self, PlatformError> {
        // `callbacks` is unused here for the same reason as on Wayland: close
        // flows through `WindowState::close` on the widget-tree side. The
        // parameter is kept so the factory keeps one construction shape.
        if !crate::x11::capabilities().supports_custom_chrome() {
            return Err(PlatformError::Unsupported);
        }
        Ok(Self { window })
    }
}

impl PlatformTitleBarHost for X11Host {
    fn reserved_leading_inset(&self) -> Size {
        Size::ZERO
    }

    fn reserved_trailing_inset(&self) -> Size {
        Size::ZERO
    }

    fn renders_custom_controls(&self) -> bool {
        true
    }

    fn needs_custom_resize_handles(&self) -> bool {
        // With `_MOTIF_WM_HINTS` decorations off the WM draws no frame and
        // provides no resize border, so the client owns the whole edge hit
        // area and hands each press back via `begin_resize`.
        true
    }

    fn begin_drag(&self) -> Result<(), PlatformError> {
        self.window
            .drag_window()
            .map_err(|e| PlatformError::Os(e.to_string()))
    }

    fn begin_resize(&self, edge: ResizeEdge) -> Result<(), PlatformError> {
        self.window
            .drag_resize_window(edge_to_direction(edge))
            .map_err(|e| PlatformError::Os(e.to_string()))
    }

    fn show_window_menu(&self, _at: Point) -> Result<(), PlatformError> {
        // Unreachable in practice: `has_window_menu` is false, so `TitleBar`
        // opens its own menu and never calls this. Reported honestly rather
        // than returning `Ok(())` for a menu that would never appear.
        Err(PlatformError::Unsupported)
    }

    fn has_window_menu(&self) -> bool {
        false
    }

    fn update_hit_regions(&self, _regions: &HitRegions) {
        // Nothing to publish: with no server-side frame every pointer event
        // reaches the widget tree, which initiates drag / resize explicitly.
        // (Windows needs this because the OS owns the non-client area.)
    }
}
