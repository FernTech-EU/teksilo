// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Window-manager capability probe for X11 client-side decorations.
//!
//! A borderless X11 window is only usable if the window manager implements
//! `_NET_WM_MOVERESIZE` — that is the *only* way a client can ask to be moved
//! or resized once it has told the WM not to draw a frame. Shipping
//! `with_decorations(false)` against a WM that lacks it produces a window the
//! user cannot move or resize at all, which is far worse than keeping native
//! decorations.
//!
//! So we probe before committing. The awkward part is ordering: winit needs the
//! decoration flag at `WindowAttributes` construction time, before any window —
//! and therefore any connection — exists, and winit exposes no way to borrow
//! its own connection early (`WindowExtX11` is an empty trait). We therefore
//! open a short-lived connection of our own, ask, and close it. The answer is
//! cached for the process.
//!
//! The probe is skipped entirely unless [`active_window_system`] predicts X11:
//! `DISPLAY` is set in virtually every Wayland session too, and some
//! compositors start XWayland lazily on the first X client connection — an
//! unconditional probe would spawn XWayland for apps that never need it.
//!
//! [`active_window_system`]: crate::window_system::active_window_system

use std::sync::OnceLock;

use x11rb::protocol::xproto::{Atom, AtomEnum, Window};

use super::connection::{X11Connection, X11Error};

/// What the running window manager can do for a borderless window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WmCapabilities {
    /// An EWMH-compliant window manager is running (validated via the two-step
    /// `_NET_SUPPORTING_WM_CHECK` handshake).
    pub ewmh_wm_present: bool,
    /// `_NET_WM_MOVERESIZE` is advertised in `_NET_SUPPORTED`.
    pub move_resize: bool,
}

impl WmCapabilities {
    /// Nothing works — the answer for "no WM", "not X11", or "probe failed".
    pub const NONE: Self = Self {
        ewmh_wm_present: false,
        move_resize: false,
    };

    /// Whether a custom title bar is safe to install. Requires both a live
    /// EWMH window manager and interactive move/resize; without either, the
    /// window would be undecorated *and* immovable.
    pub fn supports_custom_chrome(self) -> bool {
        self.ewmh_wm_present && self.move_resize
    }
}

/// Validate `_NET_SUPPORTING_WM_CHECK` per the EWMH two-step rule.
///
/// The spec requires the root window's `_NET_SUPPORTING_WM_CHECK` to name a
/// window that carries the *same* property pointing at itself. A single check
/// is not enough: when a window manager dies without cleaning up, the root
/// property survives and names a window that is gone or has been recycled, so a
/// one-step check happily reports a WM that is not running.
///
/// Returns the validated check window.
pub fn validate_wm_check(root_value: Option<u32>, child_value: Option<u32>) -> Option<Window> {
    match root_value {
        Some(win) if win != 0 && child_value == Some(win) => Some(win),
        _ => None,
    }
}

/// Whether `_NET_SUPPORTED` advertises `atom`.
pub fn supports_atom(supported: &[Atom], atom: Atom) -> bool {
    supported.contains(&atom)
}

/// Ask the running window manager what it supports. Opens (and drops) its own
/// short-lived connection.
fn probe_uncached() -> Result<WmCapabilities, X11Error> {
    let conn = X11Connection::open()?;
    let atoms = conn.atoms();

    let root_check = conn
        .get_property_full(
            conn.root(),
            atoms.net_supporting_wm_check,
            AtomEnum::WINDOW.into(),
        )?
        .and_then(|value| value.as_u32());

    // Step two: the named window must point back at itself. A `BadWindow` here
    // means the window is gone — a stale property from a crashed WM — which is
    // exactly the case this handshake exists to catch, so treat the error as
    // "no WM" rather than propagating it.
    let child_check = match root_check {
        Some(win) => conn
            .get_property_full(win, atoms.net_supporting_wm_check, AtomEnum::WINDOW.into())
            .ok()
            .flatten()
            .and_then(|value| value.as_u32()),
        None => None,
    };

    let ewmh_wm_present = validate_wm_check(root_check, child_check).is_some();
    if !ewmh_wm_present {
        return Ok(WmCapabilities::NONE);
    }

    let supported = conn
        .get_property_full(conn.root(), atoms.net_supported, AtomEnum::ATOM.into())?
        .map(|value| value.as_u32s())
        .unwrap_or_default();

    Ok(WmCapabilities {
        ewmh_wm_present,
        move_resize: supports_atom(&supported, atoms.net_wm_moveresize),
    })
}

/// The running window manager's capabilities, probed once per process.
///
/// Returns [`WmCapabilities::NONE`] when the session is not X11, when no
/// EWMH-compliant WM is running, or when the probe itself fails. Every failure
/// mode lands on "assume custom chrome is unsafe", so the worst case is a
/// window with ordinary native decorations.
///
/// A WM that starts *after* this runs (the documented GDK login-race) is not
/// re-detected; the window keeps native decorations for its lifetime.
pub fn capabilities() -> WmCapabilities {
    static CACHE: OnceLock<WmCapabilities> = OnceLock::new();
    *CACHE.get_or_init(|| {
        use crate::window_system::{WindowSystem, active_window_system};

        if active_window_system() != WindowSystem::X11 {
            return WmCapabilities::NONE;
        }
        match probe_uncached() {
            Ok(caps) => caps,
            Err(err) => {
                eprintln!(
                    "teksilo-platform: X11 window-manager probe failed ({err}); \
                     keeping native window decorations"
                );
                WmCapabilities::NONE
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wm_check_needs_the_child_to_point_at_itself() {
        assert_eq!(validate_wm_check(Some(0x200), Some(0x200)), Some(0x200));
    }

    #[test]
    fn stale_wm_check_is_rejected() {
        // A window manager that died without cleaning up leaves the root
        // property naming a window that no longer answers. One-step checking
        // would report a live WM here.
        assert_eq!(validate_wm_check(Some(0x200), None), None);
        // Recycled XID now owned by someone else, pointing elsewhere.
        assert_eq!(validate_wm_check(Some(0x200), Some(0x999)), None);
    }

    #[test]
    fn absent_wm_check_means_no_window_manager() {
        assert_eq!(validate_wm_check(None, None), None);
        assert_eq!(validate_wm_check(Some(0), Some(0)), None);
    }

    #[test]
    fn custom_chrome_needs_both_a_wm_and_move_resize() {
        assert!(
            WmCapabilities {
                ewmh_wm_present: true,
                move_resize: true
            }
            .supports_custom_chrome()
        );
        // A WM that cannot move the window would leave it stranded.
        assert!(
            !WmCapabilities {
                ewmh_wm_present: true,
                move_resize: false
            }
            .supports_custom_chrome()
        );
        // No WM at all: nothing would honour the Motif hint either.
        assert!(
            !WmCapabilities {
                ewmh_wm_present: false,
                move_resize: true
            }
            .supports_custom_chrome()
        );
        assert!(!WmCapabilities::NONE.supports_custom_chrome());
    }

    #[test]
    fn supported_atom_lookup() {
        assert!(supports_atom(&[10, 20, 30], 20));
        assert!(!supports_atom(&[10, 30], 20));
        assert!(!supports_atom(&[], 20));
    }

    /// Talks to a **real** X server, so it is `#[ignore]`d by default. This is
    /// the only way to check the probe against an actual window manager, since
    /// no in-process X server or protocol double exists for `cargo test`.
    ///
    /// ```text
    /// WAYLAND_DISPLAY= cargo test -p teksilo-platform -- --ignored --nocapture x11_probe
    /// ```
    ///
    /// Clearing `WAYLAND_DISPLAY` is what makes the session read as X11 (under
    /// a Wayland desktop this then probes XWayland's window manager).
    #[test]
    #[ignore = "requires a live X server; run with --ignored"]
    fn x11_probe_against_a_live_server() {
        let caps = probe_uncached().expect("connect to $DISPLAY");
        eprintln!("probed window manager: {caps:?}");
        assert!(
            caps.ewmh_wm_present,
            "no EWMH window manager found on $DISPLAY — custom chrome would be refused"
        );
        assert!(
            caps.move_resize,
            "_NET_WM_MOVERESIZE missing from _NET_SUPPORTED — custom chrome would be refused"
        );
    }
}
