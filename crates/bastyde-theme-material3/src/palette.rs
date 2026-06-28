// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The full Material 3 baseline role palette, attached to the theme as a
//! typed extension.
//!
//! Bastyde's [`ColorTokens`](bastyde_tokens::ColorTokens) can't hold M3's
//! complete role set (the primary/secondary/tertiary triads, every
//! `*Container` and `on*` pair, and the seven-step surface-container
//! ladder). This struct carries those values so M3-aware app code and
//! M3 widget styles can read them:
//!
//! ```ignore
//! if let Some(m3) = theme.extension::<Material3Palette>() {
//!     let tonal = m3.secondary_container;
//! }
//! ```

use bastyde_tokens::Color;

/// Full Material 3 baseline role palette (static reference scheme, seed
/// `#6750A4`). Installed on the theme via `theme.with_extension(...)` and
/// read back with `theme.extension::<Material3Palette>()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material3Palette {
    // ── Primary ──
    pub primary: Color,
    pub on_primary: Color,
    pub primary_container: Color,
    pub on_primary_container: Color,
    pub inverse_primary: Color,

    // ── Secondary ──
    pub secondary: Color,
    pub on_secondary: Color,
    pub secondary_container: Color,
    pub on_secondary_container: Color,

    // ── Tertiary ──
    pub tertiary: Color,
    pub on_tertiary: Color,
    pub tertiary_container: Color,
    pub on_tertiary_container: Color,

    // ── Error ──
    pub error: Color,
    pub on_error: Color,
    pub error_container: Color,
    pub on_error_container: Color,

    // ── Surfaces ──
    pub surface: Color,
    pub on_surface: Color,
    pub surface_variant: Color,
    pub on_surface_variant: Color,
    pub surface_container_lowest: Color,
    pub surface_container_low: Color,
    pub surface_container: Color,
    pub surface_container_high: Color,
    pub surface_container_highest: Color,
    pub surface_dim: Color,
    pub surface_bright: Color,
    pub inverse_surface: Color,
    pub inverse_on_surface: Color,

    // ── Outline / misc ──
    pub outline: Color,
    pub outline_variant: Color,
    pub scrim: Color,
    pub shadow: Color,
}

impl Material3Palette {
    /// Baseline **light** scheme.
    pub fn light() -> Self {
        Self {
            primary: Color::from_hex("#6750A4"),
            on_primary: Color::from_hex("#FFFFFF"),
            primary_container: Color::from_hex("#EADDFF"),
            on_primary_container: Color::from_hex("#21005D"),
            inverse_primary: Color::from_hex("#D0BCFF"),

            secondary: Color::from_hex("#625B71"),
            on_secondary: Color::from_hex("#FFFFFF"),
            secondary_container: Color::from_hex("#E8DEF8"),
            on_secondary_container: Color::from_hex("#1D192B"),

            tertiary: Color::from_hex("#7D5260"),
            on_tertiary: Color::from_hex("#FFFFFF"),
            tertiary_container: Color::from_hex("#FFD8E4"),
            on_tertiary_container: Color::from_hex("#31111D"),

            error: Color::from_hex("#B3261E"),
            on_error: Color::from_hex("#FFFFFF"),
            error_container: Color::from_hex("#F9DEDC"),
            on_error_container: Color::from_hex("#410E0B"),

            surface: Color::from_hex("#FEF7FF"),
            on_surface: Color::from_hex("#1D1B20"),
            surface_variant: Color::from_hex("#E7E0EC"),
            on_surface_variant: Color::from_hex("#49454F"),
            surface_container_lowest: Color::from_hex("#FFFFFF"),
            surface_container_low: Color::from_hex("#F7F2FA"),
            surface_container: Color::from_hex("#F3EDF7"),
            surface_container_high: Color::from_hex("#ECE6F0"),
            surface_container_highest: Color::from_hex("#E6E0E9"),
            surface_dim: Color::from_hex("#DED8E1"),
            surface_bright: Color::from_hex("#FEF7FF"),
            inverse_surface: Color::from_hex("#322F35"),
            inverse_on_surface: Color::from_hex("#F5EFF7"),

            outline: Color::from_hex("#79747E"),
            outline_variant: Color::from_hex("#CAC4D0"),
            scrim: Color::from_hex("#000000"),
            shadow: Color::from_hex("#000000"),
        }
    }

    /// Baseline **dark** scheme.
    pub fn dark() -> Self {
        Self {
            primary: Color::from_hex("#D0BCFF"),
            on_primary: Color::from_hex("#381E72"),
            primary_container: Color::from_hex("#4F378B"),
            on_primary_container: Color::from_hex("#EADDFF"),
            inverse_primary: Color::from_hex("#6750A4"),

            secondary: Color::from_hex("#CCC2DC"),
            on_secondary: Color::from_hex("#332D41"),
            secondary_container: Color::from_hex("#4A4458"),
            on_secondary_container: Color::from_hex("#E8DEF8"),

            tertiary: Color::from_hex("#EFB8C8"),
            on_tertiary: Color::from_hex("#492532"),
            tertiary_container: Color::from_hex("#633B48"),
            on_tertiary_container: Color::from_hex("#FFD8E4"),

            error: Color::from_hex("#F2B8B5"),
            on_error: Color::from_hex("#601410"),
            error_container: Color::from_hex("#8C1D18"),
            on_error_container: Color::from_hex("#F9DEDC"),

            surface: Color::from_hex("#141218"),
            on_surface: Color::from_hex("#E6E0E9"),
            surface_variant: Color::from_hex("#49454F"),
            on_surface_variant: Color::from_hex("#CAC4D0"),
            surface_container_lowest: Color::from_hex("#0F0D13"),
            surface_container_low: Color::from_hex("#1D1B20"),
            surface_container: Color::from_hex("#211F26"),
            surface_container_high: Color::from_hex("#2B2930"),
            surface_container_highest: Color::from_hex("#36343B"),
            surface_dim: Color::from_hex("#141218"),
            surface_bright: Color::from_hex("#3B383E"),
            inverse_surface: Color::from_hex("#E6E0E9"),
            inverse_on_surface: Color::from_hex("#322F35"),

            outline: Color::from_hex("#938F99"),
            outline_variant: Color::from_hex("#49454F"),
            scrim: Color::from_hex("#000000"),
            shadow: Color::from_hex("#000000"),
        }
    }
}
