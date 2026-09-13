// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! OS-level accessibility preference detection.
//!
//! Queries the desktop environment for user accessibility settings:
//! high contrast, reduced motion, and text scaling. Each platform uses
//! native APIs — no polling or runtime dependency beyond what the OS provides.
//!
//! # Platform support
//!
//! | Preference       | Linux (XDG portal / gsettings) | macOS (NSWorkspace)     | Windows (SystemParametersInfo / UISettings) |
//! |------------------|-------------------------------|-------------------------|---------------------------------------------|
//! | High contrast    | portal `contrast` key + GTK theme check | `accessibilityDisplayShouldIncreaseContrast` | `SPI_GETHIGHCONTRAST` |
//! | Reduced motion   | portal `reduced-motion` key + `enable-animations` | `accessibilityDisplayShouldReduceMotion` | `UISettings.AnimationsEnabled` |
//! | Text scale       | gsettings `text-scaling-factor` | N/A (uses DPI scaling) | `UISettings.TextScaleFactor` |
//! | Screen reader    | AT-SPI `org.a11y.Status.ScreenReaderEnabled` | `NSWorkspace.isVoiceOverEnabled` | `SPI_GETSCREENREADER` |
//!
//! # Why the screen-reader flag is asked of the OS and not of AccessKit
//!
//! An AccessKit adapter activates for anything that walks the accessibility
//! tree: a screen magnifier, a voice-control front end, a UI-automation
//! inspector, a tree browser. None of those want a touch to turn into an
//! explore-by-touch probe. The OS flag is the only
//! signal that distinguishes *reading the screen aloud* from *inspecting the
//! tree*, so it is the one the framework asks. Each platform's flag is
//! advisory in its own way, documented at its query below.

use teksilo_core::ScreenReaderState;

/// Accessibility preferences read from the operating system.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityPreferences {
    /// The user has enabled a high-contrast theme or mode.
    pub high_contrast: bool,
    /// The user has requested reduced or no animations.
    pub reduced_motion: bool,
    /// Text scaling factor (1.0 = normal, 1.25 = GNOME "Large Text", up to 2.25 on Windows).
    /// On macOS this is always 1.0 — text scaling is handled via display DPI.
    pub text_scale_factor: f64,
    /// Whether the OS says a screen reader is reading the screen.
    ///
    /// [`ScreenReaderState::Unknown`] when the platform does not expose the
    /// state or the query failed — which every consumer must read as "no", not
    /// as "yes".
    pub screen_reader: ScreenReaderState,
}

impl Default for AccessibilityPreferences {
    fn default() -> Self {
        Self {
            high_contrast: false,
            reduced_motion: false,
            text_scale_factor: 1.0,
            screen_reader: ScreenReaderState::Unknown,
        }
    }
}

impl AccessibilityPreferences {
    /// Query current OS accessibility preferences.
    ///
    /// This is a best-effort query. If a particular setting cannot be read
    /// (missing D-Bus service, unsupported desktop, etc.), the corresponding
    /// field falls back to its default value. Never panics.
    pub fn query() -> Self {
        platform::query()
    }

    /// Whether the user has requested larger text (text_scale_factor > 1.0).
    pub fn prefers_large_text(&self) -> bool {
        self.text_scale_factor > 1.0
    }
}

// ── Linux: XDG Desktop Portal via busctl + gsettings subprocess ─────────────
//
// Uses subprocess calls (`busctl`, `gsettings`) which are present on all major
// Linux desktops. This runs once at startup so subprocess overhead is negligible,
// and it avoids adding zbus as a direct dependency.
#[cfg(target_os = "linux")]
mod platform {
    use super::{AccessibilityPreferences, ScreenReaderState};
    use crate::linux_helpers::{read_dbus_bool_property, read_gsettings, read_portal_u32};

    pub(super) fn query() -> AccessibilityPreferences {
        let mut prefs = AccessibilityPreferences::default();

        // Try XDG Desktop Portal first (works across GNOME, KDE 6.6+, Flatpak).
        // Portal keys live under namespace "org.freedesktop.appearance".
        if let Some(v) = read_portal_u32("org.freedesktop.appearance", "contrast") {
            prefs.high_contrast = v == 1;
        }
        if let Some(v) = read_portal_u32("org.freedesktop.appearance", "reduced-motion") {
            prefs.reduced_motion = v == 1;
        }

        // High contrast fallback: check GTK theme name for "HighContrast"
        if !prefs.high_contrast
            && let Some(theme) = read_gsettings("org.gnome.desktop.interface", "gtk-theme")
        {
            prefs.high_contrast = theme.contains("HighContrast");
        }

        // High contrast fallback: GNOME a11y interface flag
        if !prefs.high_contrast
            && let Some(val) = read_gsettings("org.gnome.desktop.a11y.interface", "high-contrast")
        {
            prefs.high_contrast = val == "true";
        }

        // Reduced motion fallback: GNOME enable-animations (false → reduced motion)
        if !prefs.reduced_motion
            && let Some(val) = read_gsettings("org.gnome.desktop.interface", "enable-animations")
        {
            prefs.reduced_motion = val == "false";
        }

        // Text scaling (not in the portal, must use gsettings)
        if let Some(val) = read_gsettings("org.gnome.desktop.interface", "text-scaling-factor")
            && let Ok(scale) = val.parse::<f64>()
        {
            prefs.text_scale_factor = scale;
        }

        prefs.screen_reader = query_screen_reader();

        prefs
    }

    /// The AT-SPI status object's `ScreenReaderEnabled` flag, read off the
    /// same object `accesskit_unix` watches — `atspi-proxies`' `StatusProxy`
    /// on `org.a11y.Bus` at `/org/a11y/bus`. (AccessKit itself watches the
    /// neighbouring `IsEnabled` there, which says whether *any* client wants a
    /// tree; this is the narrower question.)
    ///
    /// Advisory: Orca sets it, and an assistive technology that never touches
    /// the status object will not. A machine with no a11y bus at all — no
    /// `busctl`, a sandbox with no session bus, a desktop that does not run
    /// one — answers `Unknown`, which behaves as "no".
    fn query_screen_reader() -> ScreenReaderState {
        match read_dbus_bool_property(
            "org.a11y.Bus",
            "/org/a11y/bus",
            "org.a11y.Status",
            "ScreenReaderEnabled",
        ) {
            Some(true) => ScreenReaderState::Active,
            Some(false) => ScreenReaderState::Inactive,
            None => ScreenReaderState::Unknown,
        }
    }
}

// ── macOS: NSWorkspace accessibility APIs ───────────────────────────────────
#[cfg(target_os = "macos")]
mod platform {
    use super::{AccessibilityPreferences, ScreenReaderState};

    pub(super) fn query() -> AccessibilityPreferences {
        let mut prefs = AccessibilityPreferences::default();

        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();

        // Available since macOS 10.10
        prefs.high_contrast = workspace.accessibilityDisplayShouldIncreaseContrast();

        // Available since macOS 10.12
        prefs.reduced_motion = workspace.accessibilityDisplayShouldReduceMotion();

        // macOS has no text-scaling API separate from DPI scaling.
        // text_scale_factor stays at 1.0 — winit's scale_factor handles DPI.

        // VoiceOver is the only screen reader on macOS, and `NSWorkspace`
        // answers for it directly (available since 10.13). There is no
        // "unknown" here: the call cannot fail, and a `false` really does mean
        // VoiceOver is off. Magnifier / Zoom and Voice Control do not set it,
        // which is exactly the discrimination we want.
        prefs.screen_reader = if workspace.isVoiceOverEnabled() {
            ScreenReaderState::Active
        } else {
            ScreenReaderState::Inactive
        };

        prefs
    }
}

// ── Windows: SystemParametersInfo + WinRT UISettings ────────────────────────
#[cfg(target_os = "windows")]
mod platform {
    use super::{AccessibilityPreferences, ScreenReaderState};

    pub(super) fn query() -> AccessibilityPreferences {
        let mut prefs = AccessibilityPreferences::default();

        prefs.high_contrast = query_high_contrast();
        let (reduced_motion, text_scale) = query_ui_settings();
        prefs.reduced_motion = reduced_motion;
        prefs.text_scale_factor = text_scale;
        prefs.screen_reader = query_screen_reader();

        prefs
    }

    /// `SPI_GETSCREENREADER` — the flag Narrator, NVDA and JAWS set while they
    /// are running.
    ///
    /// Two documented caveats, both in the direction that matters:
    /// Magnifier does not set it (so a magnifier user is not mistaken for a
    /// screen-reader user), and Windows does not reliably clear it if an
    /// assistive technology terminates without doing so itself. A stale `true`
    /// is the failure mode; nothing here can detect it, which is why the
    /// framework treats a live AccessKit *deactivation* as separate, harder
    /// evidence that no client is attached.
    fn query_screen_reader() -> ScreenReaderState {
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_GETSCREENREADER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        };

        unsafe {
            let mut on = BOOL(0);
            let ok = SystemParametersInfoW(
                SPI_GETSCREENREADER,
                0,
                Some(&mut on as *mut BOOL as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            match ok {
                Ok(()) if on.as_bool() => ScreenReaderState::Active,
                Ok(()) => ScreenReaderState::Inactive,
                // The call itself failed: we genuinely do not know.
                Err(_) => ScreenReaderState::Unknown,
            }
        }
    }

    /// Query high-contrast mode via Win32 SystemParametersInfoW.
    fn query_high_contrast() -> bool {
        use std::mem;
        use windows::Win32::UI::Accessibility::HIGHCONTRASTW;
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        };

        unsafe {
            let mut hc = HIGHCONTRASTW {
                cbSize: mem::size_of::<HIGHCONTRASTW>() as u32,
                ..Default::default()
            };
            let ok = SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                hc.cbSize,
                Some(&mut hc as *mut _ as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            if ok.is_ok() {
                // HCF_HIGHCONTRASTON = 0x00000001
                (hc.dwFlags.0 & 0x01) != 0
            } else {
                false
            }
        }
    }

    /// Query reduced motion and text scale via WinRT UISettings.
    fn query_ui_settings() -> (bool, f64) {
        use windows::UI::ViewManagement::UISettings;

        let mut reduced_motion = false;
        let mut text_scale = 1.0_f64;

        if let Ok(settings) = UISettings::new() {
            // AnimationsEnabled returns false when user has disabled animations
            if let Ok(animations_enabled) = settings.AnimationsEnabled() {
                reduced_motion = !animations_enabled;
            }

            // TextScaleFactor: 1.0 (100%) to 2.25 (225%)
            if let Ok(scale) = settings.TextScaleFactor() {
                text_scale = scale as f64;
            }
        }

        (reduced_motion, text_scale)
    }
}

// ── Fallback for other platforms (e.g., FreeBSD, Wasm) ──────────────────────
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::AccessibilityPreferences;

    /// Every field keeps its default, `screen_reader` included: this platform
    /// exposes no accessibility settings we know how to read, so `Unknown` is
    /// the honest answer rather than `Inactive`.
    pub(super) fn query() -> AccessibilityPreferences {
        AccessibilityPreferences::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preferences() {
        let prefs = AccessibilityPreferences::default();
        assert!(!prefs.high_contrast);
        assert!(!prefs.reduced_motion);
        assert!((prefs.text_scale_factor - 1.0).abs() < f64::EPSILON);
        assert!(!prefs.prefers_large_text());
        // Not `Inactive`: nobody has asked the OS yet, and the two answers
        // differ for a consumer that wants to distinguish "no screen reader"
        // from "this platform cannot say".
        assert_eq!(prefs.screen_reader, ScreenReaderState::Unknown);
    }

    #[test]
    fn large_text_threshold() {
        let mut prefs = AccessibilityPreferences {
            text_scale_factor: 1.25,
            ..AccessibilityPreferences::default()
        };
        assert!(prefs.prefers_large_text());

        prefs.text_scale_factor = 1.0;
        assert!(!prefs.prefers_large_text());
    }

    #[test]
    fn query_does_not_panic() {
        // Should never panic regardless of environment — graceful fallback.
        let prefs = AccessibilityPreferences::query();
        assert!(prefs.text_scale_factor > 0.0);
        // Whatever the host says, it is one of the three states and the query
        // returned rather than blowing up on a missing bus / API.
        assert!(matches!(
            prefs.screen_reader,
            ScreenReaderState::Unknown | ScreenReaderState::Inactive | ScreenReaderState::Active
        ));
    }

    #[test]
    fn preferences_compare_on_the_screen_reader_too() {
        // `refresh_accessibility_preferences` early-returns on `==`, so a
        // screen reader starting or stopping must count as a change or the
        // refresh would swallow it.
        let a = AccessibilityPreferences::default();
        let b = AccessibilityPreferences {
            screen_reader: ScreenReaderState::Active,
            ..AccessibilityPreferences::default()
        };
        assert_ne!(a, b);
    }
}
