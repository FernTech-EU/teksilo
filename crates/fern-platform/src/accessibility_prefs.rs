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
}

impl Default for AccessibilityPreferences {
    fn default() -> Self {
        Self {
            high_contrast: false,
            reduced_motion: false,
            text_scale_factor: 1.0,
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
    use super::AccessibilityPreferences;

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
        if !prefs.high_contrast {
            if let Some(theme) = read_gsettings("org.gnome.desktop.interface", "gtk-theme") {
                prefs.high_contrast = theme.contains("HighContrast");
            }
        }

        // High contrast fallback: GNOME a11y interface flag
        if !prefs.high_contrast {
            if let Some(val) = read_gsettings("org.gnome.desktop.a11y.interface", "high-contrast") {
                prefs.high_contrast = val == "true";
            }
        }

        // Reduced motion fallback: GNOME enable-animations (false → reduced motion)
        if !prefs.reduced_motion {
            if let Some(val) = read_gsettings("org.gnome.desktop.interface", "enable-animations") {
                prefs.reduced_motion = val == "false";
            }
        }

        // Text scaling (not in the portal, must use gsettings)
        if let Some(val) = read_gsettings("org.gnome.desktop.interface", "text-scaling-factor") {
            if let Ok(scale) = val.parse::<f64>() {
                prefs.text_scale_factor = scale;
            }
        }

        prefs
    }

    /// Read a u32 value from the XDG Desktop Portal Settings via `busctl`.
    ///
    /// The portal method `org.freedesktop.portal.Settings.ReadOne` returns
    /// a `Variant<Variant<u32>>`. `busctl` prints this as e.g. `v u 1`.
    fn read_portal_u32(namespace: &str, key: &str) -> Option<u32> {
        let output = std::process::Command::new("busctl")
            .args([
                "--user",
                "call",
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
                "ReadOne",
                "ss",
                namespace,
                key,
            ])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        // Output format: "v u <value>\n" — extract the last whitespace-separated token
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.split_whitespace().last()?.parse::<u32>().ok()
    }

    /// Read a gsettings value via the `gsettings` CLI tool.
    ///
    /// Returns the raw string output (trimmed, with surrounding quotes stripped).
    fn read_gsettings(schema: &str, key: &str) -> Option<String> {
        let output = std::process::Command::new("gsettings")
            .args(["get", schema, key])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // gsettings wraps strings in single quotes: 'Adwaita'
        Some(value.trim_matches('\'').to_string())
    }
}

// ── macOS: NSWorkspace accessibility APIs ───────────────────────────────────
#[cfg(target_os = "macos")]
mod platform {
    use super::AccessibilityPreferences;

    pub(super) fn query() -> AccessibilityPreferences {
        let mut prefs = AccessibilityPreferences::default();

        // Safety: NSWorkspace.sharedWorkspace is always available on the main
        // thread. These accessibilityDisplay* methods are simple property reads
        // that do not mutate state.
        unsafe {
            let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();

            // Available since macOS 10.10
            prefs.high_contrast = workspace.accessibilityDisplayShouldIncreaseContrast();

            // Available since macOS 10.12
            prefs.reduced_motion = workspace.accessibilityDisplayShouldReduceMotion();
        }

        // macOS has no text-scaling API separate from DPI scaling.
        // text_scale_factor stays at 1.0 — winit's scale_factor handles DPI.

        prefs
    }
}

// ── Windows: SystemParametersInfo + WinRT UISettings ────────────────────────
#[cfg(target_os = "windows")]
mod platform {
    use super::AccessibilityPreferences;

    pub(super) fn query() -> AccessibilityPreferences {
        let mut prefs = AccessibilityPreferences::default();

        prefs.high_contrast = query_high_contrast();
        let (reduced_motion, text_scale) = query_ui_settings();
        prefs.reduced_motion = reduced_motion;
        prefs.text_scale_factor = text_scale;

        prefs
    }

    /// Query high-contrast mode via Win32 SystemParametersInfoW.
    fn query_high_contrast() -> bool {
        use std::mem;
        use windows::Win32::UI::Accessibility::HIGHCONTRASTW;
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_GETHIGHCONTRAST, SystemParametersInfoW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
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
    }

    #[test]
    fn large_text_threshold() {
        let mut prefs = AccessibilityPreferences::default();
        prefs.text_scale_factor = 1.25;
        assert!(prefs.prefers_large_text());

        prefs.text_scale_factor = 1.0;
        assert!(!prefs.prefers_large_text());
    }

    #[test]
    fn query_does_not_panic() {
        // Should never panic regardless of environment — graceful fallback.
        let prefs = AccessibilityPreferences::query();
        assert!(prefs.text_scale_factor > 0.0);
    }
}
