// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! OS-reported theme colors — pure data types with no platform logic.
//!
//! Populated by `teksilo-platform::os_theme`, consumed by
//! `ColorTokens::from_os_colors()` to build a full theme from partial OS data.

use crate::Color;

/// The user's preferred color scheme as reported by the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorSchemePreference {
    /// The OS reports a light theme preference.
    Light,
    /// The OS reports a dark theme preference.
    Dark,
    /// No preference reported (treat as light).
    #[default]
    NoPreference,
}

impl ColorSchemePreference {
    /// Whether the preference is dark.
    pub fn is_dark(self) -> bool {
        self == Self::Dark
    }
}

/// Colors read from the OS desktop environment.
///
/// All fields except `color_scheme` are `Option`: the factory method
/// `ColorTokens::from_os_colors()` fills missing values from the matching
/// built-in light or dark theme, and an absent field leaves its tokens
/// exactly as that theme has them.
///
/// The struct is `#[non_exhaustive]` so a desktop reader can learn to report
/// a new colour without breaking anyone: outside this crate, start from
/// [`OsThemeColors::new`] (or `Default`) and assign the fields you have.
///
/// Each field names where it comes from: the KDE colour-scheme key
/// (`[Colors:<set>] <Role>` in `kdeglobals`) and, where one exists, the GTK
/// CSS `@define-color` name a GTK theme publishes for it.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct OsThemeColors {
    /// Light or dark preference.
    pub color_scheme: ColorSchemePreference,
    /// Desktop accent / primary color. KDE `[Colors:Selection]
    /// BackgroundNormal` (the colour the portal reports as `accent-color`);
    /// GNOME's accent colour.
    pub accent: Option<Color>,
    /// Window background color.
    pub window_bg: Option<Color>,
    /// Window foreground (text) color.
    pub window_fg: Option<Color>,
    /// Button background color.
    pub button_bg: Option<Color>,
    /// Button foreground color.
    pub button_fg: Option<Color>,
    /// Selection / highlight background.
    pub selection_bg: Option<Color>,
    /// Selection foreground.
    pub selection_fg: Option<Color>,
    /// Tooltip background.
    pub tooltip_bg: Option<Color>,
    /// Tooltip foreground.
    pub tooltip_fg: Option<Color>,
    /// Background of a content view: a list, a tree, a text field, an
    /// editor. KDE `[Colors:View] BackgroundNormal`; GTK `theme_base_color`
    /// (libadwaita `view_bg_color`).
    pub view_bg: Option<Color>,
    /// Text of a content view. KDE `[Colors:View] ForegroundNormal`; GTK
    /// `theme_text_color` (libadwaita `view_fg_color`).
    pub view_fg: Option<Color>,
    /// The alternate background of a striped list or table row. KDE
    /// `[Colors:View] BackgroundAlternate`; GTK publishes no equivalent.
    pub alternate_bg: Option<Color>,
    /// Secondary text: less important labels, comments, placeholder-like
    /// hints. Not disabled text. KDE `[Colors:View] ForegroundInactive`; GTK
    /// publishes no equivalent (its `insensitive_fg_color` is disabled text).
    pub inactive_fg: Option<Color>,
    /// An unvisited link. KDE `[Colors:View] ForegroundLink`; GTK
    /// `link_color`.
    pub link: Option<Color>,
    /// A visited link. KDE `[Colors:View] ForegroundVisited`; GTK
    /// `link_visited_color`.
    pub link_visited: Option<Color>,
    /// Error text and indicators. KDE `[Colors:View] ForegroundNegative`;
    /// GTK `error_color`.
    pub error: Option<Color>,
    /// Warning text and indicators. KDE `[Colors:View] ForegroundNeutral`;
    /// GTK `warning_color`.
    pub warning: Option<Color>,
    /// Success text and indicators. KDE `[Colors:View] ForegroundPositive`;
    /// GTK `success_color`.
    pub success: Option<Color>,
    /// The keyboard focus indicator. KDE `[Colors:View] DecorationFocus`;
    /// GTK publishes no standard name (the Breeze GTK theme calls it
    /// `theme_button_decoration_focus_breeze`).
    pub focus: Option<Color>,
}

impl OsThemeColors {
    /// A report carrying only the light or dark preference, every colour
    /// absent. The way to build one outside this crate, since the struct is
    /// `#[non_exhaustive]`; assign the colours the desktop reports afterwards.
    pub fn new(color_scheme: ColorSchemePreference) -> Self {
        Self {
            color_scheme,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_dark() {
        assert!(ColorSchemePreference::Dark.is_dark());
        assert!(!ColorSchemePreference::Light.is_dark());
        assert!(!ColorSchemePreference::NoPreference.is_dark());
    }

    #[test]
    fn new_carries_the_scheme_and_no_colours() {
        let os = OsThemeColors::new(ColorSchemePreference::Dark);
        assert_eq!(os.color_scheme, ColorSchemePreference::Dark);
        assert_eq!(
            os,
            OsThemeColors {
                color_scheme: ColorSchemePreference::Dark,
                ..OsThemeColors::default()
            }
        );
        assert_eq!(
            OsThemeColors::new(ColorSchemePreference::NoPreference),
            OsThemeColors::default()
        );
    }
}
