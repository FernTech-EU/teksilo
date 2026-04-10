use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::motion::MotionTokens;
use crate::os_theme_colors::OsThemeColors;
use crate::shape::ShapeTokens;
use crate::spacing::SpacingTokens;
use crate::typography::TypographyTokens;

/// All semantic color tokens for a theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorTokens {
    // Surface colors
    pub surface: Color,
    pub surface_secondary: Color,
    pub surface_tertiary: Color,
    pub on_surface: Color,
    pub on_surface_secondary: Color,

    // Primary
    pub primary: Color,
    pub primary_hover: Color,
    pub primary_pressed: Color,
    pub on_primary: Color,

    // Secondary
    pub secondary: Color,
    pub secondary_hover: Color,
    pub secondary_pressed: Color,
    pub on_secondary: Color,

    // Status
    pub error: Color,
    pub on_error: Color,
    pub warning: Color,
    pub on_warning: Color,
    pub success: Color,
    pub on_success: Color,
    pub info: Color,
    pub on_info: Color,

    // Interaction & utility
    pub disabled_fill: Color,
    pub disabled_text: Color,
    pub focus_ring: Color,
    pub border: Color,
    pub border_strong: Color,
    pub scrim: Color,
    pub tooltip_surface: Color,
    pub tooltip_text: Color,
    pub selection: Color,
    pub on_selection: Color,
    pub highlight: Color,
}

impl ColorTokens {
    pub fn light_default() -> Self {
        Self {
            surface: Color::from_hex("#FAFAFA"),
            surface_secondary: Color::from_hex("#F5F5F5"),
            surface_tertiary: Color::from_hex("#EEEEEE"),
            on_surface: Color::from_hex("#212121"),
            on_surface_secondary: Color::from_hex("#616161"),

            primary: Color::from_hex("#1565C0"),
            primary_hover: Color::from_hex("#0D47A1"),
            primary_pressed: Color::from_hex("#0A3A82"),
            on_primary: Color::WHITE,

            secondary: Color::from_hex("#546E7A"),
            secondary_hover: Color::from_hex("#455A64"),
            secondary_pressed: Color::from_hex("#37474F"),
            on_secondary: Color::WHITE,

            error: Color::from_hex("#D32F2F"),
            on_error: Color::WHITE,
            warning: Color::from_hex("#F57C00"),
            on_warning: Color::from_hex("#3E2723"),
            success: Color::from_hex("#388E3C"),
            on_success: Color::WHITE,
            info: Color::from_hex("#1976D2"),
            on_info: Color::WHITE,

            disabled_fill: Color::from_hex("#E0E0E0"),
            disabled_text: Color::from_hex("#9E9E9E"),
            focus_ring: Color::from_rgba(0.08, 0.40, 0.75, 0.75),
            border: Color::from_hex("#E0E0E0"),
            border_strong: Color::from_hex("#BDBDBD"),
            scrim: Color::new(0.0, 0.0, 0.0, 0.32),
            tooltip_surface: Color::from_hex("#616161"),
            tooltip_text: Color::WHITE,
            selection: Color::from_rgba(0.08, 0.40, 0.75, 0.2),
            on_selection: Color::from_hex("#212121"),
            highlight: Color::from_rgba(1.0, 0.92, 0.23, 0.3),
        }
    }

    pub fn dark_default() -> Self {
        Self {
            surface: Color::from_hex("#121212"),
            surface_secondary: Color::from_hex("#1E1E1E"),
            surface_tertiary: Color::from_hex("#2C2C2C"),
            on_surface: Color::from_hex("#E0E0E0"),
            on_surface_secondary: Color::from_hex("#A0A0A0"),

            primary: Color::from_hex("#64B5F6"),
            primary_hover: Color::from_hex("#90CAF9"),
            primary_pressed: Color::from_hex("#42A5F5"),
            on_primary: Color::from_hex("#0D2137"),

            secondary: Color::from_hex("#80CBC4"),
            secondary_hover: Color::from_hex("#A7D8D2"),
            secondary_pressed: Color::from_hex("#4DB6AC"),
            on_secondary: Color::from_hex("#0D2625"),

            error: Color::from_hex("#EF9A9A"),
            on_error: Color::from_hex("#3B0D0D"),
            warning: Color::from_hex("#FFB74D"),
            on_warning: Color::from_hex("#3B2400"),
            success: Color::from_hex("#66BB6A"),
            on_success: Color::from_hex("#0D2E0F"),
            info: Color::from_hex("#64B5F6"),
            on_info: Color::from_hex("#0D2137"),

            disabled_fill: Color::from_hex("#333333"),
            disabled_text: Color::from_hex("#666666"),
            focus_ring: Color::from_rgba(0.39, 0.71, 0.96, 0.75),
            border: Color::from_hex("#333333"),
            border_strong: Color::from_hex("#555555"),
            scrim: Color::new(0.0, 0.0, 0.0, 0.64),
            tooltip_surface: Color::from_hex("#424242"),
            tooltip_text: Color::from_hex("#E0E0E0"),
            selection: Color::from_rgba(0.39, 0.71, 0.96, 0.25),
            on_selection: Color::from_hex("#E0E0E0"),
            highlight: Color::from_rgba(1.0, 0.92, 0.23, 0.2),
        }
    }

    /// Build a full color token set from OS-reported colors.
    ///
    /// Starts from the matching built-in theme (light or dark) based on
    /// `color_scheme`, then overlays any color fields that were actually
    /// read from the OS. Missing fields keep their built-in default values.
    pub fn from_os_colors(os: &OsThemeColors) -> Self {
        let is_dark = os.color_scheme.is_dark();
        let mut tokens = if is_dark {
            Self::dark_default()
        } else {
            Self::light_default()
        };

        // Accent → primary family + focus/selection/info
        if let Some(accent) = os.accent {
            tokens.primary = accent;
            tokens.primary_hover = if is_dark {
                accent.lighten(0.10)
            } else {
                accent.darken(0.10)
            };
            tokens.primary_pressed = if is_dark {
                accent.lighten(0.20)
            } else {
                accent.darken(0.20)
            };
            // Pick on_primary for contrast: white on dark accents, dark on light accents
            tokens.on_primary = if accent.relative_luminance() > 0.4 {
                Color::from_hex("#212121")
            } else {
                Color::WHITE
            };
            tokens.focus_ring = accent.with_alpha(0.75);
            tokens.selection = accent.with_alpha(0.20);
            tokens.info = accent;
            tokens.on_info = tokens.on_primary;
        }

        // Window background → surface family
        if let Some(bg) = os.window_bg {
            tokens.surface = bg;
            tokens.surface_secondary = if is_dark {
                bg.lighten(0.04)
            } else {
                bg.darken(0.02)
            };
            tokens.surface_tertiary = if is_dark {
                bg.lighten(0.08)
            } else {
                bg.darken(0.04)
            };
        }

        // Window foreground → on_surface family
        if let Some(fg) = os.window_fg {
            tokens.on_surface = fg;
            tokens.on_surface_secondary = if is_dark {
                fg.darken(0.25)
            } else {
                fg.lighten(0.25)
            };
        }

        // Button colors → secondary family
        if let Some(btn_bg) = os.button_bg {
            tokens.secondary = btn_bg;
            tokens.secondary_hover = if is_dark {
                btn_bg.lighten(0.10)
            } else {
                btn_bg.darken(0.10)
            };
            tokens.secondary_pressed = if is_dark {
                btn_bg.lighten(0.20)
            } else {
                btn_bg.darken(0.20)
            };
        }
        if let Some(btn_fg) = os.button_fg {
            tokens.on_secondary = btn_fg;
        }

        // Selection colors
        if let Some(sel_bg) = os.selection_bg {
            tokens.selection = sel_bg.with_alpha(0.25);
            tokens.on_selection = os.selection_fg.unwrap_or(tokens.on_surface);
            // If no accent was provided, derive it from the selection color
            if os.accent.is_none() {
                tokens.primary = sel_bg;
                tokens.primary_hover = if is_dark {
                    sel_bg.lighten(0.10)
                } else {
                    sel_bg.darken(0.10)
                };
                tokens.primary_pressed = if is_dark {
                    sel_bg.lighten(0.20)
                } else {
                    sel_bg.darken(0.20)
                };
                tokens.on_primary = if sel_bg.relative_luminance() > 0.4 {
                    Color::from_hex("#212121")
                } else {
                    Color::WHITE
                };
                tokens.focus_ring = sel_bg.with_alpha(0.75);
                tokens.info = sel_bg;
                tokens.on_info = tokens.on_primary;
            }
        }

        // Tooltip colors
        if let Some(tt_bg) = os.tooltip_bg {
            tokens.tooltip_surface = tt_bg;
        }
        if let Some(tt_fg) = os.tooltip_fg {
            tokens.tooltip_text = tt_fg;
        }

        // Derive border from surface if we have OS surfaces
        if os.window_bg.is_some() || os.window_fg.is_some() {
            tokens.border = tokens.on_surface.with_alpha(0.12);
            tokens.border_strong = tokens.on_surface.with_alpha(0.25);
            tokens.disabled_fill = tokens.on_surface.with_alpha(0.08);
            tokens.disabled_text = tokens.on_surface.with_alpha(0.38);
            tokens.scrim = Color::new(0.0, 0.0, 0.0, if is_dark { 0.64 } else { 0.32 });
        }

        tokens
    }
}

/// The complete theme containing all design tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub colors: ColorTokens,
    pub spacing: SpacingTokens,
    pub typography: TypographyTokens,
    pub shape: ShapeTokens,
    pub motion: MotionTokens,
}

impl Theme {
    pub fn light_default() -> Self {
        Self {
            colors: ColorTokens::light_default(),
            spacing: SpacingTokens::default(),
            typography: TypographyTokens::default(),
            shape: ShapeTokens::default(),
            motion: MotionTokens::default(),
        }
    }

    pub fn dark_default() -> Self {
        Self {
            colors: ColorTokens::dark_default(),
            spacing: SpacingTokens::default(),
            typography: TypographyTokens::default(),
            shape: ShapeTokens::default(),
            motion: MotionTokens::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_default_has_distinct_primary_and_surface() {
        let theme = Theme::light_default();
        assert_ne!(theme.colors.primary, theme.colors.surface);
    }

    #[test]
    fn dark_default_has_distinct_primary_and_surface() {
        let theme = Theme::dark_default();
        assert_ne!(theme.colors.primary, theme.colors.surface);
    }

    #[test]
    fn light_and_dark_have_different_surfaces() {
        let light = Theme::light_default();
        let dark = Theme::dark_default();
        assert_ne!(light.colors.surface, dark.colors.surface);
    }

    #[test]
    fn theme_serde_roundtrip() {
        let theme = Theme::light_default();
        let json = serde_json::to_string(&theme).unwrap();
        let deserialized: Theme = serde_json::from_str(&json).unwrap();
        assert_eq!(theme.colors.primary, deserialized.colors.primary);
        assert_eq!(theme.spacing, deserialized.spacing);
    }

    #[test]
    fn color_tokens_on_primary_contrasts_with_primary() {
        let colors = ColorTokens::light_default();
        // on_primary should be white or very light for a dark primary
        assert_ne!(colors.primary, colors.on_primary);
    }

    #[test]
    fn from_os_colors_no_colors_returns_light_default() {
        let os = OsThemeColors::default(); // NoPreference, all None
        let tokens = ColorTokens::from_os_colors(&os);
        let light = ColorTokens::light_default();
        assert_eq!(tokens.surface, light.surface);
        assert_eq!(tokens.primary, light.primary);
    }

    #[test]
    fn from_os_colors_dark_scheme_returns_dark_base() {
        let os = OsThemeColors {
            color_scheme: crate::os_theme_colors::ColorSchemePreference::Dark,
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        let dark = ColorTokens::dark_default();
        assert_eq!(tokens.surface, dark.surface);
    }

    #[test]
    fn from_os_colors_accent_overrides_primary() {
        let accent = Color::from_hex("#FF5500");
        let os = OsThemeColors {
            accent: Some(accent),
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        assert_eq!(tokens.primary, accent);
        // on_primary should contrast with the accent
        assert_ne!(tokens.primary, tokens.on_primary);
        // Focus ring should derive from accent
        assert!((tokens.focus_ring.a() - 0.75).abs() < 0.01);
    }

    #[test]
    fn from_os_colors_window_bg_overrides_surface() {
        let bg = Color::from_hex("#AABBCC");
        let os = OsThemeColors {
            window_bg: Some(bg),
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        assert_eq!(tokens.surface, bg);
        // Secondary/tertiary should be derived, not equal
        assert_ne!(tokens.surface, tokens.surface_secondary);
        assert_ne!(tokens.surface, tokens.surface_tertiary);
    }

    #[test]
    fn from_os_colors_selection_derives_primary_when_no_accent() {
        let sel = Color::from_hex("#3DAEE9");
        let os = OsThemeColors {
            selection_bg: Some(sel),
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        // Without an explicit accent, primary should be derived from selection
        assert_eq!(tokens.primary, sel);
    }

    #[test]
    fn from_os_colors_accent_takes_precedence_over_selection() {
        let accent = Color::from_hex("#FF0000");
        let sel = Color::from_hex("#00FF00");
        let os = OsThemeColors {
            accent: Some(accent),
            selection_bg: Some(sel),
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        // Accent should win for primary
        assert_eq!(tokens.primary, accent);
    }

    #[test]
    fn from_os_colors_tooltip_colors() {
        let tt_bg = Color::from_hex("#333333");
        let tt_fg = Color::from_hex("#EEEEEE");
        let os = OsThemeColors {
            tooltip_bg: Some(tt_bg),
            tooltip_fg: Some(tt_fg),
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        assert_eq!(tokens.tooltip_surface, tt_bg);
        assert_eq!(tokens.tooltip_text, tt_fg);
    }
}
