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
/// All fields except `color_scheme` are `Option` — the factory method
/// `ColorTokens::from_os_colors()` fills missing values from the matching
/// built-in light or dark theme.
#[derive(Debug, Clone, Default)]
pub struct OsThemeColors {
    /// Light or dark preference.
    pub color_scheme: ColorSchemePreference,
    /// Desktop accent / primary color (e.g., GNOME accent, KDE DecorationFocus).
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
}
