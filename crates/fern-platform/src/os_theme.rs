//! OS theme detection — reads colors directly from desktop environment
//! configuration files and settings.
//!
//! Supports GNOME, KDE, and Cinnamon on Linux. macOS and Windows return
//! only the light/dark preference (via winit), with no color reading.

use fern_tokens::{Color, ColorSchemePreference, OsThemeColors};

/// Query only the OS light/dark preference (lightweight).
/// Used by `ThemeMode::FollowSystem`.
pub fn query_color_scheme() -> ColorSchemePreference {
    platform::query_color_scheme()
}

/// Query full OS theme colors from desktop config files.
/// Used by `ThemeMode::Native`.
pub fn query_os_theme_colors() -> OsThemeColors {
    platform::query_os_theme_colors()
}

// ── Linux ───────────────────────────────────────────────────────────────────
#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use crate::linux_helpers::{
        Desktop, detect_desktop, read_gsettings, read_portal_rgb, read_portal_u32,
    };

    pub(super) fn query_color_scheme() -> ColorSchemePreference {
        // XDG portal: 0 = no preference, 1 = dark, 2 = light
        if let Some(v) = read_portal_u32("org.freedesktop.appearance", "color-scheme") {
            return match v {
                1 => ColorSchemePreference::Dark,
                2 => ColorSchemePreference::Light,
                _ => ColorSchemePreference::NoPreference,
            };
        }

        // Fallback: infer from GTK theme name
        let theme_name = match detect_desktop() {
            Desktop::Cinnamon => read_gsettings("org.cinnamon.desktop.interface", "gtk-theme"),
            _ => read_gsettings("org.gnome.desktop.interface", "gtk-theme"),
        };

        if let Some(name) = theme_name {
            if name.to_lowercase().contains("dark") {
                return ColorSchemePreference::Dark;
            }
            return ColorSchemePreference::Light;
        }

        ColorSchemePreference::NoPreference
    }

    pub(super) fn query_os_theme_colors() -> OsThemeColors {
        let mut colors = OsThemeColors {
            color_scheme: query_color_scheme(),
            ..Default::default()
        };

        match detect_desktop() {
            Desktop::Kde => query_kde(&mut colors),
            Desktop::Cinnamon => query_cinnamon(&mut colors),
            Desktop::Gnome | Desktop::Other(_) => query_gnome(&mut colors),
        }

        colors
    }

    // ── GNOME ────────────────────────────────────────────────────────────

    fn query_gnome(colors: &mut OsThemeColors) {
        // Accent color from XDG portal (RGB doubles)
        if let Some((r, g, b)) = read_portal_rgb("org.freedesktop.appearance", "accent-color") {
            colors.accent = Some(Color::from_rgb(r as f32, g as f32, b as f32));
        }

        // Fallback: GNOME 47 named accent
        if colors.accent.is_none() {
            if let Some(name) = read_gsettings("org.gnome.desktop.interface", "accent-color") {
                colors.accent = gnome_named_accent(&name);
            }
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

    // ── KDE ──────────────────────────────────────────────────────────────

    fn query_kde(colors: &mut OsThemeColors) {
        let path = dirs_kdeglobals();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return,
        };

        // Infer dark/light from color scheme name if portal didn't provide it
        if colors.color_scheme == ColorSchemePreference::NoPreference {
            if let Some(scheme) = ini_value(&content, "General", "ColorScheme") {
                if scheme.to_lowercase().contains("dark") {
                    colors.color_scheme = ColorSchemePreference::Dark;
                } else {
                    colors.color_scheme = ColorSchemePreference::Light;
                }
            }
        }

        // Window colors
        colors.window_bg = ini_color(&content, "Colors:Window", "BackgroundNormal");
        colors.window_fg = ini_color(&content, "Colors:Window", "ForegroundNormal");
        colors.accent = ini_color(&content, "Colors:Window", "DecorationFocus");

        // Button colors
        colors.button_bg = ini_color(&content, "Colors:Button", "BackgroundNormal");
        colors.button_fg = ini_color(&content, "Colors:Button", "ForegroundNormal");

        // Selection colors
        colors.selection_bg = ini_color(&content, "Colors:Selection", "BackgroundNormal");
        colors.selection_fg = ini_color(&content, "Colors:Selection", "ForegroundNormal");

        // Tooltip colors
        colors.tooltip_bg = ini_color(&content, "Colors:Tooltip", "BackgroundNormal");
        colors.tooltip_fg = ini_color(&content, "Colors:Tooltip", "ForegroundNormal");
    }

    fn dirs_kdeglobals() -> std::path::PathBuf {
        if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
            std::path::PathBuf::from(config_home).join("kdeglobals")
        } else if let Ok(home) = std::env::var("HOME") {
            std::path::PathBuf::from(home).join(".config/kdeglobals")
        } else {
            std::path::PathBuf::from("/dev/null")
        }
    }

    /// Read a value from a simple INI file (section + key).
    fn ini_value<'a>(content: &'a str, section: &str, key: &str) -> Option<&'a str> {
        let section_header = format!("[{}]", section);
        let mut in_section = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_section = trimmed == section_header;
                continue;
            }
            if in_section {
                if let Some((k, v)) = trimmed.split_once('=') {
                    if k.trim() == key {
                        return Some(v.trim());
                    }
                }
            }
        }
        None
    }

    /// Parse a KDE "R,G,B" color value (0-255 integers).
    fn ini_color(content: &str, section: &str, key: &str) -> Option<Color> {
        let value = ini_value(content, section, key)?;
        let parts: Vec<&str> = value.split(',').collect();
        if parts.len() >= 3 {
            let r = parts[0].trim().parse::<u8>().ok()?;
            let g = parts[1].trim().parse::<u8>().ok()?;
            let b = parts[2].trim().parse::<u8>().ok()?;
            Some(Color::from_rgb(
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
            ))
        } else {
            None
        }
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

        // Derive accent from selection color if not already set
        if colors.accent.is_none() {
            if let Some(sel) = resolve(&["theme_selected_bg_color"]) {
                colors.accent = Some(sel);
            }
        }
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
        fn kde_ini_parsing() {
            let content = r#"
[General]
ColorScheme=Breeze-Dark

[Colors:Window]
BackgroundNormal=49,54,59
ForegroundNormal=239,240,241
DecorationFocus=61,174,233
"#;
            let bg = ini_color(content, "Colors:Window", "BackgroundNormal").unwrap();
            assert!((bg.r() - 49.0 / 255.0).abs() < 0.01);

            let fg = ini_color(content, "Colors:Window", "ForegroundNormal").unwrap();
            assert!((fg.r() - 239.0 / 255.0).abs() < 0.01);

            let scheme = ini_value(content, "General", "ColorScheme").unwrap();
            assert!(scheme.contains("Dark"));
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
        // macOS color scheme is best read from winit's window.theme() at startup.
        // Without a window, we cannot query it here.
        // TODO: use NSAppearance.current.name via objc2 for windowless detection.
        ColorSchemePreference::NoPreference
    }

    pub(super) fn query_os_theme_colors() -> OsThemeColors {
        OsThemeColors {
            color_scheme: query_color_scheme(),
            ..Default::default()
        }
    }
}

// ── Windows ─────────────────────────────────────────────────────────────────
#[cfg(target_os = "windows")]
mod platform {
    use super::*;

    pub(super) fn query_color_scheme() -> ColorSchemePreference {
        // TODO: read HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme
        // 0 = dark, 1 = light
        ColorSchemePreference::NoPreference
    }

    pub(super) fn query_os_theme_colors() -> OsThemeColors {
        OsThemeColors {
            color_scheme: query_color_scheme(),
            ..Default::default()
        }
    }
}

// ── Fallback ────────────────────────────────────────────────────────────────
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::*;

    pub(super) fn query_color_scheme() -> ColorSchemePreference {
        ColorSchemePreference::NoPreference
    }

    pub(super) fn query_os_theme_colors() -> OsThemeColors {
        OsThemeColors::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_color_scheme_does_not_panic() {
        let _ = query_color_scheme();
    }

    #[test]
    fn query_os_theme_colors_does_not_panic() {
        let colors = query_os_theme_colors();
        // Should at least return a valid color scheme preference
        let _ = colors.color_scheme;
    }
}
