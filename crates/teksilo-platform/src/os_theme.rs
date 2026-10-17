// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! OS theme detection — reads colors directly from desktop environment
//! configuration files and settings.
//!
//! Supports GNOME, KDE, and Cinnamon on Linux. macOS and Windows return
//! only the light/dark preference (via winit), with no color reading.
//!
//! On a KDE Plasma session ([`teksilo_kde_scheme::is_kde_session`]) the
//! colours are KDE's own: the `kdeglobals` cascade, with its system layers
//! and `kdedefaults`, and the colour scheme in effect with the user's
//! accent, read through `teksilo-kde-scheme`. [`query_native_desktop`]
//! also hands over the KDE desktop's settings (fonts, animation speed,
//! widget style, Breeze style settings) as a [`KdeDesktop`].
//!
//! An [`OsThemeWatch`] hears these change while the application runs; see
//! [`watch`].

#[cfg(target_os = "linux")]
pub(crate) mod kde;
pub mod watch;

pub use watch::{OsThemeChanged, OsThemeWatch};

use teksilo_kde_scheme::KdeDesktop;
#[cfg(target_os = "linux")]
use teksilo_tokens::Color;
use teksilo_tokens::{ColorSchemePreference, OsThemeColors};

/// What the desktop reports for `ThemeMode::Native`: its colours, and on a
/// KDE Plasma session its desktop settings.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct NativeDesktop {
    /// The desktop's colour report.
    pub os: OsThemeColors,
    /// The KDE desktop's colour scheme and settings, on a KDE session whose
    /// `kdeglobals` could be read; `None` elsewhere.
    ///
    /// When present, `os` is its scheme's colour report
    /// ([`KdeColorScheme::os_theme_colors`](teksilo_kde_scheme::KdeColorScheme::os_theme_colors)).
    pub kde: Option<KdeDesktop>,
}

impl NativeDesktop {
    /// A desktop reporting `os` and no KDE settings.
    pub fn new(os: OsThemeColors) -> Self {
        NativeDesktop { os, kde: None }
    }
}

/// Query only the OS light/dark preference (lightweight).
/// Used by `ThemeMode::FollowSystem`.
///
/// On Linux the desktop portal is asked first. When it does not answer, a
/// KDE session answers from its colour scheme (dark when the window
/// background is dark, the rule KDE reports to applications), and GNOME and
/// Cinnamon from their GTK theme's name.
pub fn query_color_scheme() -> ColorSchemePreference {
    platform::query_color_scheme()
}

/// Query the desktop's colours and, on a KDE Plasma session, its settings.
/// Used by `ThemeMode::Native`.
///
/// On a KDE session the colours are the KDE colour scheme's
/// ([`KdeColorScheme::os_theme_colors`](teksilo_kde_scheme::KdeColorScheme::os_theme_colors)),
/// accent and light or dark preference included; only when no
/// `kdeglobals` file can be read does the report fall back to the desktop
/// portal's preference and accent colour, with no [`KdeDesktop`]. Elsewhere
/// the report is the one [`query_os_theme_colors`] has always given.
pub fn query_native_desktop() -> NativeDesktop {
    platform::query_native_desktop()
}

/// Query full OS theme colors from desktop config files.
/// Used by `ThemeMode::Native`.
///
/// The colour report of [`query_native_desktop`].
pub fn query_os_theme_colors() -> OsThemeColors {
    query_native_desktop().os
}

// ── Linux ───────────────────────────────────────────────────────────────────
#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use crate::linux_helpers::{Desktop, detect_desktop, read_gsettings};
    use crate::linux_portal::{read_portal_rgb, read_portal_u32};
    use teksilo_kde_scheme::KdeConfigLocations;

    pub(super) fn query_color_scheme() -> ColorSchemePreference {
        // XDG portal: 0 = no preference, 1 = dark, 2 = light.
        // Works on GNOME and on KDE Plasma 5.27+ (with the
        // `xdg-desktop-portal-kde` package installed). On older or
        // headless KDE setups the portal call returns None and we
        // fall through.
        if let Some(v) = read_portal_u32("org.freedesktop.appearance", "color-scheme") {
            return match v {
                1 => ColorSchemePreference::Dark,
                2 => ColorSchemePreference::Light,
                _ => ColorSchemePreference::NoPreference,
            };
        }

        // Desktop-specific fallback. Picks the right config source
        // for the current DE rather than blindly trying GNOME's
        // gsettings (which is empty on a clean KDE install and
        // would otherwise leave us at `NoPreference`, mapping
        // dark-mode KDE users to a light theme).
        match detect_desktop() {
            Desktop::Kde => {
                // The scheme in effect decides, by its window background,
                // as KDE reports it to applications and to the portal.
                let locations = KdeConfigLocations::from_env(kde::process_env);
                if let Some(dark) =
                    kde::kde_prefers_dark(&locations, &kde::process_env, kde::read_file)
                {
                    return if dark {
                        ColorSchemePreference::Dark
                    } else {
                        ColorSchemePreference::Light
                    };
                }
            }
            Desktop::Cinnamon => {
                if let Some(name) = read_gsettings("org.cinnamon.desktop.interface", "gtk-theme") {
                    if name.to_lowercase().contains("dark") {
                        return ColorSchemePreference::Dark;
                    }
                    return ColorSchemePreference::Light;
                }
            }
            Desktop::Gnome | Desktop::Other(_) => {
                if let Some(name) = read_gsettings("org.gnome.desktop.interface", "gtk-theme") {
                    if name.to_lowercase().contains("dark") {
                        return ColorSchemePreference::Dark;
                    }
                    return ColorSchemePreference::Light;
                }
            }
        }

        ColorSchemePreference::NoPreference
    }

    pub(super) fn query_native_desktop() -> NativeDesktop {
        match detect_desktop() {
            Desktop::Kde => {
                let locations = KdeConfigLocations::from_env(kde::process_env);
                let desktop = kde::discover_kde(&locations, &kde::process_env, kde::read_file);
                kde::native_desktop(desktop, || {
                    let mut colors = OsThemeColors::new(query_color_scheme());
                    colors.accent = portal_accent();
                    colors
                })
            }
            Desktop::Cinnamon => {
                let mut colors = OsThemeColors::new(query_color_scheme());
                query_cinnamon(&mut colors);
                NativeDesktop::new(colors)
            }
            Desktop::Gnome | Desktop::Other(_) => {
                let mut colors = OsThemeColors::new(query_color_scheme());
                query_gnome(&mut colors);
                NativeDesktop::new(colors)
            }
        }
    }

    /// The desktop portal's accent colour (RGB doubles).
    fn portal_accent() -> Option<Color> {
        read_portal_rgb("org.freedesktop.appearance", "accent-color")
            .map(|(r, g, b)| Color::from_rgb(r as f32, g as f32, b as f32))
    }

    // ── GNOME ────────────────────────────────────────────────────────────

    fn query_gnome(colors: &mut OsThemeColors) {
        // Accent color from XDG portal (RGB doubles)
        colors.accent = portal_accent();

        // Fallback: GNOME 47 named accent
        if colors.accent.is_none()
            && let Some(name) = read_gsettings("org.gnome.desktop.interface", "accent-color")
        {
            colors.accent = gnome_named_accent(&name);
        }

        // Read surface/selection colors from GTK CSS
        if let Some(theme_name) = read_gsettings("org.gnome.desktop.interface", "gtk-theme") {
            apply_gtk_css_colors(colors, &theme_name);
        }
    }

    /// Map GNOME 47 named accent colors to RGB.
    fn gnome_named_accent(name: &str) -> Option<Color> {
        let hex = match name.to_lowercase().as_str() {
            "blue" => "#3584e4",
            "teal" => "#2190a4",
            "green" => "#3a944a",
            "yellow" => "#c88800",
            "orange" => "#ed5b00",
            "red" => "#e62d42",
            "pink" => "#d56199",
            "purple" => "#9141ac",
            "slate" => "#6f8396",
            _ => return None,
        };
        Some(Color::from_hex(hex))
    }

    // ── Cinnamon ─────────────────────────────────────────────────────────

    fn query_cinnamon(colors: &mut OsThemeColors) {
        let theme_name = read_gsettings("org.cinnamon.desktop.interface", "gtk-theme")
            .or_else(|| read_gsettings("org.gnome.desktop.interface", "gtk-theme"));

        if let Some(ref name) = theme_name {
            // Infer accent from Mint-Y theme name suffix
            colors.accent = mint_y_accent(name);

            // Read surface/selection colors from GTK CSS
            apply_gtk_css_colors(colors, name);
        }
    }

    /// Map Mint-Y theme name suffixes to accent colors.
    fn mint_y_accent(theme_name: &str) -> Option<Color> {
        // Theme names like "Mint-Y-Dark-Aqua" → extract the last segment
        let suffix = theme_name.rsplit('-').next()?;
        let hex = match suffix.to_lowercase().as_str() {
            "aqua" => "#1a9e87",
            "blue" => "#0c75de",
            "grey" => "#70737a",
            "orange" => "#dd6516",
            "pink" => "#e54980",
            "purple" => "#7e57c2",
            "red" => "#c0392b",
            "sand" => "#c5a07c",
            "teal" => "#009688",
            // Default Mint-Y (no accent suffix) uses green
            _ if theme_name.starts_with("Mint-Y") => "#92b372",
            _ => return None,
        };
        Some(Color::from_hex(hex))
    }

    // ── Shared GTK CSS parser ────────────────────────────────────────────

    /// Parse `@define-color` declarations from a GTK theme's CSS and apply
    /// well-known color names to the `OsThemeColors` struct.
    fn apply_gtk_css_colors(colors: &mut OsThemeColors, theme_name: &str) {
        let css = load_gtk_css(theme_name);
        if css.is_empty() {
            return;
        }

        let defined = parse_define_colors(&css);

        // Resolve well-known GTK color names to actual Color values.
        // Try both bare names and DE-specific suffixed names (e.g. `_breeze`).
        let resolve = |names: &[&str]| -> Option<Color> {
            for name in names {
                if let Some(c) = resolve_color(&defined, name) {
                    return Some(c);
                }
            }
            None
        };

        if colors.window_bg.is_none() {
            colors.window_bg = resolve(&["theme_bg_color"]);
        }
        if colors.window_fg.is_none() {
            colors.window_fg = resolve(&["theme_fg_color"]);
        }
        if colors.selection_bg.is_none() {
            colors.selection_bg = resolve(&["theme_selected_bg_color"]);
        }
        if colors.selection_fg.is_none() {
            colors.selection_fg = resolve(&["theme_selected_fg_color"]);
        }
        if colors.button_bg.is_none() {
            colors.button_bg =
                resolve(&["theme_button_background_normal", "theme_unfocused_bg_color"]);
        }
        if colors.tooltip_bg.is_none() {
            colors.tooltip_bg = resolve(&["tooltip_bg_color"]);
        }
        if colors.tooltip_fg.is_none() {
            colors.tooltip_fg = resolve(&["tooltip_fg_color"]);
        }
        fill_gtk_view_colors(colors, resolve);

        // Derive accent from selection color if not already set
        if colors.accent.is_none()
            && let Some(sel) = resolve(&["theme_selected_bg_color"])
        {
            colors.accent = Some(sel);
        }
    }

    /// Fill the content-view and semantic colours from the GTK names that
    /// carry them, leaving any a desktop reader already set. GTK themes
    /// publish no alternate-row or secondary-text colour (its
    /// `insensitive_fg_color` is disabled text, not secondary), so
    /// `alternate_bg` and `inactive_fg` stay `None` here. Breeze GTK names its
    /// links and its focus decoration only with a `_breeze` suffix, hence the
    /// second name in those lists.
    fn fill_gtk_view_colors(
        colors: &mut OsThemeColors,
        resolve: impl Fn(&[&str]) -> Option<Color>,
    ) {
        let fill = |slot: &mut Option<Color>, names: &[&str]| {
            if slot.is_none() {
                *slot = resolve(names);
            }
        };
        fill(&mut colors.view_bg, &["theme_base_color", "view_bg_color"]);
        fill(&mut colors.view_fg, &["theme_text_color", "view_fg_color"]);
        fill(&mut colors.link, &["link_color", "link_color_breeze"]);
        fill(
            &mut colors.link_visited,
            &["link_visited_color", "link_visited_color_breeze"],
        );
        fill(&mut colors.error, &["error_color"]);
        fill(&mut colors.warning, &["warning_color"]);
        fill(&mut colors.success, &["success_color"]);
        fill(&mut colors.focus, &["theme_button_decoration_focus_breeze"]);
    }

    /// Load GTK CSS for a theme. Tries gtk-4.0 first, falls back to gtk-3.0.
    /// Also checks user override directories.
    fn load_gtk_css(theme_name: &str) -> String {
        let candidates = [
            // User themes
            format!(
                "{}/.themes/{}/gtk-4.0/gtk.css",
                std::env::var("HOME").unwrap_or_default(),
                theme_name
            ),
            format!(
                "{}/.themes/{}/gtk-3.0/gtk.css",
                std::env::var("HOME").unwrap_or_default(),
                theme_name
            ),
            // System themes
            format!("/usr/share/themes/{}/gtk-4.0/gtk.css", theme_name),
            format!("/usr/share/themes/{}/gtk-3.0/gtk.css", theme_name),
            // Flatpak/snap locations
            format!("/usr/local/share/themes/{}/gtk-4.0/gtk.css", theme_name),
        ];

        for path in &candidates {
            if let Ok(content) = std::fs::read_to_string(path) {
                // Skip placeholder files (e.g., Adwaita's "this file is no longer used")
                if content.contains("@define-color") {
                    return content;
                }
            }
        }

        String::new()
    }

    /// Parse all `@define-color name value;` declarations from GTK CSS.
    /// Returns a map of name → raw value string.
    fn parse_define_colors(css: &str) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for line in css.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("@define-color") {
                let rest = rest.trim();
                if let Some((name, value)) = rest.split_once(char::is_whitespace) {
                    let value = value.trim().trim_end_matches(';').trim();
                    map.insert(name.to_string(), value.to_string());
                }
            }
        }
        map
    }

    /// Resolve a GTK CSS color name to a `Color`, following `@name` references.
    fn resolve_color(
        defined: &std::collections::HashMap<String, String>,
        name: &str,
    ) -> Option<Color> {
        resolve_color_depth(defined, name, 0)
    }

    fn resolve_color_depth(
        defined: &std::collections::HashMap<String, String>,
        name: &str,
        depth: u32,
    ) -> Option<Color> {
        if depth > 10 {
            return None; // prevent infinite loops
        }

        let value = defined.get(name)?;

        // Reference to another color: @other_name
        if let Some(ref_name) = value.strip_prefix('@') {
            return resolve_color_depth(defined, ref_name.trim(), depth + 1);
        }

        parse_css_color(value)
    }

    /// Parse a CSS color value: #hex, rgb(...), rgba(...), or named color.
    fn parse_css_color(value: &str) -> Option<Color> {
        let value = value.trim();

        // #rrggbb or #rrggbbaa
        if value.starts_with('#') {
            return Some(Color::from_hex(value));
        }

        // rgba(r, g, b, a) — values are 0-255 integers or percentages
        if let Some(inner) = value
            .strip_prefix("rgba(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() == 4 {
                let r = parse_css_component(parts[0])?;
                let g = parse_css_component(parts[1])?;
                let b = parse_css_component(parts[2])?;
                let a = parts[3].trim().parse::<f32>().ok()?;
                return Some(Color::from_rgba(r, g, b, a));
            }
        }

        // rgb(r, g, b)
        if let Some(inner) = value.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() == 3 {
                let r = parse_css_component(parts[0])?;
                let g = parse_css_component(parts[1])?;
                let b = parse_css_component(parts[2])?;
                return Some(Color::from_rgb(r, g, b));
            }
        }

        // Named colors
        match value.to_lowercase().as_str() {
            "white" => Some(Color::WHITE),
            "black" => Some(Color::BLACK),
            "transparent" => Some(Color::TRANSPARENT),
            _ => None,
        }
    }

    /// Parse a CSS color component: integer (0-255) or float (already 0.0-1.0).
    /// GTK CSS uses integer 0-255 for rgb/rgba channels.
    fn parse_css_component(s: &str) -> Option<f32> {
        let s = s.trim();
        if s.contains('.') {
            // Fractional value — treat as 0.0-1.0 range
            s.parse::<f32>().ok()
        } else if let Ok(i) = s.parse::<u16>() {
            // Integer — treat as 0-255 range
            Some(i as f32 / 255.0)
        } else {
            None
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parse_define_colors_basic() {
            let css = r#"
@define-color theme_bg_color #eff0f1;
@define-color theme_fg_color #232629;
@define-color theme_selected_bg_color @accent_color;
@define-color accent_color #3584e4;
"#;
            let defined = parse_define_colors(css);
            assert_eq!(defined.get("theme_bg_color").unwrap(), "#eff0f1");
            assert_eq!(defined.get("theme_fg_color").unwrap(), "#232629");

            let c = resolve_color(&defined, "theme_selected_bg_color").unwrap();
            assert!((c.r() - 0x35 as f32 / 255.0).abs() < 0.01);
        }

        #[test]
        fn gtk_view_colors_come_from_their_css_names() {
            // A cut-down Breeze GTK theme: the bare names reference the
            // `_breeze` ones, and links and focus exist only suffixed.
            let css = r#"
@define-color theme_base_color_breeze #ffffff;
@define-color theme_text_color_breeze #232629;
@define-color link_color_breeze #2980b9;
@define-color link_visited_color_breeze #9b59b6;
@define-color error_color_breeze #da4453;
@define-color warning_color_breeze #f67400;
@define-color success_color_breeze #27ae60;
@define-color theme_button_decoration_focus_breeze  #3daee9;
@define-color theme_base_color @theme_base_color_breeze;
@define-color theme_text_color @theme_text_color_breeze;
@define-color error_color @error_color_breeze;
@define-color warning_color @warning_color_breeze;
@define-color success_color @success_color_breeze;
"#;
            let defined = parse_define_colors(css);
            let resolve = |names: &[&str]| names.iter().find_map(|n| resolve_color(&defined, n));
            let mut colors = OsThemeColors::new(ColorSchemePreference::Light);
            // A colour a desktop reader already set is kept.
            colors.success = Some(Color::BLACK);
            fill_gtk_view_colors(&mut colors, resolve);
            assert_eq!(colors.view_bg, Some(Color::from_hex("#ffffff")));
            assert_eq!(colors.view_fg, Some(Color::from_hex("#232629")));
            assert_eq!(colors.link, Some(Color::from_hex("#2980b9")));
            assert_eq!(colors.link_visited, Some(Color::from_hex("#9b59b6")));
            assert_eq!(colors.error, Some(Color::from_hex("#da4453")));
            assert_eq!(colors.warning, Some(Color::from_hex("#f67400")));
            assert_eq!(colors.success, Some(Color::BLACK));
            assert_eq!(colors.focus, Some(Color::from_hex("#3daee9")));
            // GTK carries neither of these.
            assert_eq!(colors.alternate_bg, None);
            assert_eq!(colors.inactive_fg, None);

            // A theme without the names leaves every field absent.
            let empty = std::collections::HashMap::new();
            let mut colors = OsThemeColors::new(ColorSchemePreference::Dark);
            fill_gtk_view_colors(&mut colors, |names: &[&str]| {
                names.iter().find_map(|n| resolve_color(&empty, n))
            });
            assert_eq!(colors, OsThemeColors::new(ColorSchemePreference::Dark));
        }

        #[test]
        fn parse_css_color_hex() {
            let c = parse_css_color("#3584e4").unwrap();
            assert!((c.r() - 0x35 as f32 / 255.0).abs() < 0.01);
            assert!((c.g() - 0x84 as f32 / 255.0).abs() < 0.01);
        }

        #[test]
        fn parse_css_color_rgba() {
            let c = parse_css_color("rgba(61, 174, 233, 0.5)").unwrap();
            assert!((c.r() - 61.0 / 255.0).abs() < 0.01);
            assert!((c.a() - 0.5).abs() < 0.01);
        }

        #[test]
        fn parse_css_color_named() {
            assert_eq!(parse_css_color("white").unwrap(), Color::WHITE);
            assert_eq!(parse_css_color("black").unwrap(), Color::BLACK);
        }

        #[test]
        fn gnome_named_accent_mapping() {
            assert!(gnome_named_accent("blue").is_some());
            assert!(gnome_named_accent("teal").is_some());
            assert!(gnome_named_accent("nonexistent").is_none());
        }

        #[test]
        fn mint_y_accent_mapping() {
            assert!(mint_y_accent("Mint-Y-Dark-Aqua").is_some());
            assert!(mint_y_accent("Mint-Y-Blue").is_some());
            assert!(mint_y_accent("Mint-Y").is_some());
            assert!(mint_y_accent("Adwaita").is_none());
        }
    }
}

// ── macOS ───────────────────────────────────────────────────────────────────
#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    pub(super) fn query_color_scheme() -> ColorSchemePreference {
        // Windowless color-scheme detection isn't implemented on macOS yet; the
        // reliable source is winit's `window.theme()` at/after window creation
        // (the app layer should drive `ThemeMode::Native` from that). Warn once
        // in debug so this fallback isn't silent.
        // TODO: use NSAppearance.current.name via objc2 for windowless detection.
        #[cfg(debug_assertions)]
        {
            use std::sync::Once;
            static WARNED: Once = Once::new();
            WARNED.call_once(|| {
                eprintln!(
                    "teksilo-platform: ThemeMode::Native windowless color-scheme \
                     detection is unimplemented on macOS — falling back to \
                     NoPreference; drive it from winit's window.theme() instead."
                );
            });
        }
        ColorSchemePreference::NoPreference
    }

    pub(super) fn query_native_desktop() -> NativeDesktop {
        NativeDesktop::new(OsThemeColors::new(query_color_scheme()))
    }
}

// ── Windows ─────────────────────────────────────────────────────────────────
#[cfg(target_os = "windows")]
mod platform {
    use super::*;

    pub(super) fn query_color_scheme() -> ColorSchemePreference {
        // Windowless color-scheme detection isn't implemented on Windows yet
        // (UI_ViewManagement::UISettings or the AppsUseLightTheme registry key
        // would do it); winit's `window.theme()` is the reliable path. Warn
        // once in debug so this fallback isn't silent.
        // TODO: read HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme
        // 0 = dark, 1 = light
        #[cfg(debug_assertions)]
        {
            use std::sync::Once;
            static WARNED: Once = Once::new();
            WARNED.call_once(|| {
                eprintln!(
                    "teksilo-platform: ThemeMode::Native windowless color-scheme \
                     detection is unimplemented on Windows — falling back to \
                     NoPreference; drive it from winit's window.theme() instead."
                );
            });
        }
        ColorSchemePreference::NoPreference
    }

    pub(super) fn query_native_desktop() -> NativeDesktop {
        NativeDesktop::new(OsThemeColors::new(query_color_scheme()))
    }
}

// ── Fallback ────────────────────────────────────────────────────────────────
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::*;

    pub(super) fn query_color_scheme() -> ColorSchemePreference {
        ColorSchemePreference::NoPreference
    }

    pub(super) fn query_native_desktop() -> NativeDesktop {
        NativeDesktop::new(OsThemeColors::default())
    }
}
