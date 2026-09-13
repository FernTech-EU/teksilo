// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The part of a window a person cannot fully see or touch.
//!
//! A window owns a rectangle; it does not always *get* all of it. A display
//! cutout eats the top, a rounded corner clips the corners, a home indicator
//! reserves the bottom. Those are platform facts, and like the reduced-motion
//! preference they have to reach the widget tree from outside it — the tree
//! cannot deduce a notch.
//!
//! # What each desktop platform reports, and why three of them report nothing
//!
//! - **macOS** — real. `NSView.safeAreaInsets` (macOS 11+) returns the
//!   camera-housing inset on a 14"/16" MacBook Pro. In an ordinary windowed
//!   app the menu bar already covers the housing and the answer is zero; it
//!   becomes non-zero when the window occupies that band, which in practice
//!   means full screen. This is the one desktop platform that answers, and
//!   [`window_safe_area`] asks it directly.
//! - **Windows** — nothing to report. There is no cutout, and the Win11 DWM
//!   rounds the *frame*, not the client area a client-decorated window paints
//!   into; no API reports a client-area safe inset.
//! - **Wayland** — nothing to report. Neither xdg-shell nor xdg-decoration nor
//!   any stable protocol carries a display cutout.
//! - **X11** — nothing to report. No core-protocol request, no XInput2
//!   property and no EWMH hint says one.
//!
//! Those three zeroes are **answers**, not to-dos: the same shape as the
//! `false`s in [`BackendCaps::for_platform`](crate::pointer_backend::BackendCaps::for_platform).
//! Each is stated at the branch that returns it.
//!
//! # Sides, not leading and trailing
//!
//! [`SafeAreaSides`] names physical edges — `left` and `right`, not `leading`
//! and `trailing` — because that is what a platform reports and it is the
//! widget tree, which knows the layout direction, that decides which is which.
//! [`SafeAreaSides::to_insets`] does the mapping.

use teksilo_canvas::EdgeInsets;

/// Safe-area insets as the platform reports them: by physical edge, in logical
/// pixels.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct SafeAreaSides {
    /// Inset from the top edge — a notch, a camera housing, a status bar.
    pub top: f32,
    /// Inset from the bottom edge — a home indicator.
    pub bottom: f32,
    /// Inset from the physical left edge.
    pub left: f32,
    /// Inset from the physical right edge.
    pub right: f32,
}

impl SafeAreaSides {
    /// Nothing is inset: the window is usable to its last pixel.
    pub const ZERO: Self = Self {
        top: 0.0,
        bottom: 0.0,
        left: 0.0,
        right: 0.0,
    };

    /// Whether every side is zero — the desktop answer on three of four
    /// platforms, and on the fourth whenever the window is not spanning the
    /// housing.
    pub fn is_zero(&self) -> bool {
        *self == Self::ZERO
    }

    /// Map the physical edges onto the tree's RTL-aware [`EdgeInsets`].
    ///
    /// Under [`LeftToRight`](teksilo_core::environment::LayoutDirection::LeftToRight)
    /// `leading` is the left edge; under
    /// [`RightToLeft`](teksilo_core::environment::LayoutDirection::RightToLeft)
    /// it is the right one. The vertical pair never mirrors.
    pub fn to_insets(self, direction: teksilo_core::environment::LayoutDirection) -> EdgeInsets {
        let rtl = matches!(
            direction,
            teksilo_core::environment::LayoutDirection::RightToLeft
        );
        let (leading, trailing) = if rtl {
            (self.right, self.left)
        } else {
            (self.left, self.right)
        };
        EdgeInsets {
            top: self.top,
            bottom: self.bottom,
            leading,
            trailing,
        }
    }
}

/// The safe area of `window`, in logical pixels.
///
/// [`SafeAreaSides::ZERO`] wherever the platform reports nothing, which is
/// every desktop platform but macOS — see the module docs for the reason at
/// each one.
pub fn window_safe_area(window: &winit::window::Window) -> SafeAreaSides {
    #[cfg(target_os = "macos")]
    {
        macos::safe_area(window)
    }
    // Windows: no cutout, and the rounded frame is the DWM's, not the client
    // area's. Wayland: no protocol carries a cutout. X11: likewise.
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        SafeAreaSides::ZERO
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::SafeAreaSides;
    use objc2::rc::Retained;
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// **Verification status:** written against objc2-app-kit 0.3
    /// (`NSView::safeAreaInsets`, macOS 11+). Not exercised on a notched Mac
    /// in full screen — that is the P44 hardware sign-off. On every other Mac,
    /// and in every windowed frame, AppKit's own answer is zero, which is what
    /// makes the untested case the *non*-default one.
    pub(super) fn safe_area(window: &winit::window::Window) -> SafeAreaSides {
        let Ok(handle) = window.window_handle() else {
            return SafeAreaSides::ZERO;
        };
        let RawWindowHandle::AppKit(raw) = handle.as_raw() else {
            return SafeAreaSides::ZERO;
        };
        // Re-retain the NSView pointer winit hands us — the same pattern the
        // title-bar host and the drag destination use.
        let Some(view): Option<Retained<NSView>> =
            (unsafe { Retained::retain(raw.ns_view.as_ptr().cast()) })
        else {
            return SafeAreaSides::ZERO;
        };
        // AppKit reports insets in *points*, which are Teksilo's logical
        // pixels, so no scale conversion belongs here.
        // `NSView::safeAreaInsets` is a *safe* `pub fn` in objc2-app-kit 0.3
        // (no argument to get wrong, no ownership to hand over), so no `unsafe`
        // block belongs here; the retain above is the one call that needs one.
        let insets = view.safeAreaInsets();
        SafeAreaSides {
            top: insets.top as f32,
            bottom: insets.bottom as f32,
            left: insets.left as f32,
            right: insets.right as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::environment::LayoutDirection;

    #[test]
    fn zero_is_zero_on_every_side() {
        assert!(SafeAreaSides::ZERO.is_zero());
        assert!(SafeAreaSides::default().is_zero());
        assert!(
            !SafeAreaSides {
                top: 1.0,
                ..SafeAreaSides::ZERO
            }
            .is_zero()
        );
    }

    #[test]
    fn a_left_inset_leads_in_ltr_and_trails_in_rtl() {
        let sides = SafeAreaSides {
            top: 32.0,
            bottom: 8.0,
            left: 5.0,
            right: 3.0,
        };
        let ltr = sides.to_insets(LayoutDirection::LeftToRight);
        assert_eq!(ltr.leading, 5.0);
        assert_eq!(ltr.trailing, 3.0);
        let rtl = sides.to_insets(LayoutDirection::RightToLeft);
        assert_eq!(rtl.leading, 3.0);
        assert_eq!(rtl.trailing, 5.0);
        // The vertical pair is not a direction question.
        assert_eq!((ltr.top, ltr.bottom), (32.0, 8.0));
        assert_eq!((rtl.top, rtl.bottom), (32.0, 8.0));
    }
}
