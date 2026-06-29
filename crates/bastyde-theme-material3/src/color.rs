// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Material 3 baseline color tokens mapped onto Bastyde's [`ColorTokens`].
//!
//! Values are the static Material 3 reference scheme derived from the
//! canonical seed `#6750A4` (the "baseline" purple). Bastyde's
//! [`ColorTokens`] uses a role taxonomy (surfaces / accent / borders /
//! status) that doesn't line up one-to-one with M3's
//! primary/secondary/tertiary + tonal-surface model, so each field is
//! mapped to its nearest M3 equivalent. The richer M3 role set that has
//! no `ColorTokens` home (containers, secondary, tertiary, the full
//! surface-container ladder) is exposed separately via
//! [`crate::palette::Material3Palette`].
//!
//! M3 hover/pressed states are *compositional* (an 8 % / 12 % on-color
//! "state layer" over the base). Bastyde recipes take opaque colors, so
//! we pre-composite those layers with [`Color::mix`]. Disabled colors use
//! M3's opacity model directly via [`Color::with_alpha`].
//!
//! Fields with no M3 equivalent (info/success/warning status, search
//! match, scrollbars, the secondary editor colors, the chart palette)
//! keep their IntUI defaults — we start from `ColorTokens::*_default()`
//! and override only the M3-relevant fields.

use bastyde_tokens::{Color, ColorTokens};

/// Material 3 baseline **light** color tokens.
pub fn m3_light_colors() -> ColorTokens {
    // ── M3 baseline reference roles (seed #6750A4), light scheme ──
    let primary = Color::from_hex("#6750A4");
    let on_primary = Color::from_hex("#FFFFFF");
    let primary_container = Color::from_hex("#EADDFF");
    let secondary_container = Color::from_hex("#E8DEF8");
    let tertiary = Color::from_hex("#7D5260");
    let error = Color::from_hex("#B3261E");
    let on_error = Color::from_hex("#FFFFFF");
    let error_container = Color::from_hex("#F9DEDC");
    let on_error_container = Color::from_hex("#410E0B");
    let surface = Color::from_hex("#FEF7FF");
    let on_surface = Color::from_hex("#1D1B20");
    let on_surface_variant = Color::from_hex("#49454F");
    let surface_variant = Color::from_hex("#E7E0EC");
    let outline = Color::from_hex("#79747E");
    let outline_variant = Color::from_hex("#CAC4D0");
    let surface_container_lowest = Color::from_hex("#FFFFFF");
    let surface_container_low = Color::from_hex("#F7F2FA");
    let surface_container = Color::from_hex("#F3EDF7");
    let surface_container_high = Color::from_hex("#ECE6F0");
    let inverse_surface = Color::from_hex("#322F35");
    let inverse_on_surface = Color::from_hex("#F5EFF7");

    let mut c = ColorTokens::light_default();
    apply_m3_roles(
        &mut c,
        M3Roles {
            primary,
            on_primary,
            primary_container,
            secondary_container,
            tertiary,
            error,
            on_error,
            error_container,
            on_error_container,
            surface,
            on_surface,
            on_surface_variant,
            surface_variant,
            outline,
            outline_variant,
            surface_container_lowest,
            surface_container_low,
            surface_container,
            surface_container_high,
            inverse_surface,
            inverse_on_surface,
        },
        // Light: link hover darkens the (dark) primary.
        primary.darken(0.1),
    );
    c
}

/// Material 3 baseline **dark** color tokens.
pub fn m3_dark_colors() -> ColorTokens {
    // ── M3 baseline reference roles (seed #6750A4), dark scheme ──
    let primary = Color::from_hex("#D0BCFF");
    let on_primary = Color::from_hex("#381E72");
    let primary_container = Color::from_hex("#4F378B");
    let secondary_container = Color::from_hex("#4A4458");
    let tertiary = Color::from_hex("#EFB8C8");
    let error = Color::from_hex("#F2B8B5");
    let on_error = Color::from_hex("#601410");
    let error_container = Color::from_hex("#8C1D18");
    let on_error_container = Color::from_hex("#F9DEDC");
    let surface = Color::from_hex("#141218");
    let on_surface = Color::from_hex("#E6E0E9");
    let on_surface_variant = Color::from_hex("#CAC4D0");
    let surface_variant = Color::from_hex("#49454F");
    let outline = Color::from_hex("#938F99");
    let outline_variant = Color::from_hex("#49454F");
    let surface_container_lowest = Color::from_hex("#0F0D13");
    let surface_container_low = Color::from_hex("#1D1B20");
    let surface_container = Color::from_hex("#211F26");
    let surface_container_high = Color::from_hex("#2B2930");
    let inverse_surface = Color::from_hex("#E6E0E9");
    let inverse_on_surface = Color::from_hex("#322F35");

    let mut c = ColorTokens::dark_default();
    apply_m3_roles(
        &mut c,
        M3Roles {
            primary,
            on_primary,
            primary_container,
            secondary_container,
            tertiary,
            error,
            on_error,
            error_container,
            on_error_container,
            surface,
            on_surface,
            on_surface_variant,
            surface_variant,
            outline,
            outline_variant,
            surface_container_lowest,
            surface_container_low,
            surface_container,
            surface_container_high,
            inverse_surface,
            inverse_on_surface,
        },
        // Dark: link hover lightens the (light) primary.
        primary.lighten(0.1),
    );
    c
}

/// The subset of M3 reference roles needed to drive a `ColorTokens` mapping.
struct M3Roles {
    primary: Color,
    on_primary: Color,
    primary_container: Color,
    secondary_container: Color,
    tertiary: Color,
    error: Color,
    on_error: Color,
    error_container: Color,
    on_error_container: Color,
    surface: Color,
    on_surface: Color,
    on_surface_variant: Color,
    surface_variant: Color,
    outline: Color,
    outline_variant: Color,
    surface_container_lowest: Color,
    surface_container_low: Color,
    surface_container: Color,
    surface_container_high: Color,
    inverse_surface: Color,
    inverse_on_surface: Color,
}

/// Shared light/dark mapping — the appearance difference lives entirely in
/// the M3 role values passed in, so the field-by-field projection is one
/// function. `link_hover` is the only value that flips direction between
/// schemes (darken vs lighten the primary), so it is passed explicitly.
fn apply_m3_roles(c: &mut ColorTokens, m: M3Roles, link_hover: Color) {
    // ── Surfaces ──
    c.surface_main = m.surface;
    c.surface_content = m.surface_container_low;
    c.surface_raised = m.surface_container_high;
    c.surface_sunken = m.surface_container_lowest;
    // M3 state layers: hover = 8 % onSurface, pressed = 12 % onSurface.
    c.surface_hover = m.surface.mix(m.on_surface, 0.08);
    c.surface_pressed = m.surface.mix(m.on_surface, 0.12);
    c.surface_selected = m.secondary_container;
    c.surface_selected_inactive = m.surface_variant;
    c.surface_alt_row = m.surface_container;

    // ── Text ──
    c.text_primary = m.on_surface;
    c.text_secondary = m.on_surface_variant;
    c.text_disabled = m.on_surface.with_alpha(0.38); // M3 disabled = 38 % opacity
    c.text_on_accent = m.on_primary;
    c.text_link = m.primary;
    c.text_link_hover = link_hover;
    c.text_link_visited = m.tertiary;
    c.text_error = m.error;

    // ── Accent (M3 primary) ──
    c.accent = m.primary;
    c.accent_hover = m.primary.mix(m.on_primary, 0.08);
    c.accent_pressed = m.primary.mix(m.on_primary, 0.12);
    c.accent_disabled = m.on_surface.with_alpha(0.12); // M3 disabled container
    c.accent_subtle_bg = m.primary_container;

    // ── Borders / dividers ──
    c.border = m.outline_variant; // M3 default borders use outlineVariant
    c.border_strong = m.outline;
    c.border_focused = m.primary;
    c.border_error = m.error;
    c.divider = m.outline_variant;
    c.divider_strong = m.outline;

    // ── Cross-language container / on-error roles (now first-class in core) ──
    c.text_on_error = m.on_error;
    c.text_on_error_container = m.on_error_container;
    c.surface_error_container = m.error_container;
    c.surface_container = m.surface_container;
    c.surface_container_raised = m.surface_container_high;
    c.surface_container_sunken = m.surface_container_low;

    // ── Status — only error has an M3 role; info/success/warning keep IntUI ──
    c.status_error_fg = m.error;
    c.status_error_bg = m.error_container;

    // ── Selection ──
    c.selection_bg_active = m.primary;
    c.selection_text_active = m.on_primary;
    c.selection_bg_inactive = m.surface_container_high;
    c.selection_text_inactive = m.on_surface_variant;

    // ── Tooltip — M3 inverseSurface (light-on-dark in light theme) ──
    c.tooltip_bg = m.inverse_surface;
    c.tooltip_text = m.inverse_on_surface;
    c.tooltip_border = Color::TRANSPARENT; // M3 tooltips have no border
    c.tooltip_shortcut = m.inverse_on_surface.with_alpha(0.7);

    // ── Editor ──
    c.editor_bg = m.surface;
    c.editor_fg = m.on_surface;
    c.editor_caret = m.primary;
    c.editor_selection_bg = m.secondary_container;

    // ── Misc ──
    c.focus_ring = m.primary;
    c.focus_ring_error = m.error;
}
