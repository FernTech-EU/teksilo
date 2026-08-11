// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! WinUI 3 theme-resource colours projected onto Teksilo's [`ColorTokens`].
//!
//! The source values live in [`crate::palette::FluentPalette`] (transcribed
//! verbatim from WinUI's `Common_themeresources_any.xaml`); this module is
//! the *projection* — one Fluent token chosen per Teksilo role, plus the
//! handful of derivations Fluent has no token for.
//!
//! ## Translucency
//!
//! Most Fluent fills are deliberately translucent: a control fill is a
//! wash over whatever layer it sits on, and stacking `SolidBackground →
//! Layer → ControlFill` is how the design language builds depth. Teksilo
//! paints flat colours over the parent surface, which composites the same
//! way — so alpha is **kept** wherever the wash is the point (strokes,
//! hover / pressed / selection washes, disabled fills, the scrim) and
//! **pre-composited** wherever a Teksilo slot must be opaque (window and
//! editor backgrounds, the accent fill family whose contrast against
//! on-accent text has to be predictable). [`crate::palette::over`] does
//! the compositing.
//!
//! ## What Fluent has no token for
//!
//! Fluent's dictionary is control-centric; a few Teksilo roles have no
//! counterpart and are derived here, each marked at its assignment:
//! `accent_subtle_bg`, `text_link_visited`, the editor's current-line and
//! code-block colours, and the scroll-bar thumb's hover / pressed steps
//! (WinUI animates the thumb's *size*, not its colour). The search-match
//! highlight keeps the IntUI baseline — an amber wash reads correctly on
//! both Fluent appearances and Fluent specifies nothing for it.

use teksilo_tokens::{Color, ColorTokens};

use crate::palette::{FluentPalette, over};

/// Fluent **light** colour tokens, from the WinUI `"Light"` dictionary.
pub fn fluent_light_colors(p: &FluentPalette) -> ColorTokens {
    let mut c = ColorTokens::light_default();
    apply(&mut c, p, Appearance::Light);
    c
}

/// Fluent **dark** colour tokens, from the WinUI `"Default"` dictionary.
pub fn fluent_dark_colors(p: &FluentPalette) -> ColorTokens {
    let mut c = ColorTokens::dark_default();
    apply(&mut c, p, Appearance::Dark);
    c
}

/// The two places where the projection genuinely branches on appearance —
/// everywhere else the difference already lives in the palette values.
#[derive(Copy, Clone, PartialEq)]
enum Appearance {
    Light,
    Dark,
}

fn apply(c: &mut ColorTokens, p: &FluentPalette, appearance: Appearance) {
    let light = appearance == Appearance::Light;
    let base = p.solid_background_base;

    // ── Surfaces ───────────────────────────────────────────────────────
    // The Fluent depth ladder: the page is SolidBackgroundFillColorBase,
    // a control fill sits on it, a flyout floats above on the tertiary
    // solid (the documented opaque fallback for its acrylic backdrop),
    // and a well recesses to the secondary solid.
    c.surface_main = base;
    c.surface_content = p.control_fill_default_solid();
    c.surface_raised = p.solid_background_tertiary;
    c.surface_sunken = p.solid_background_secondary;
    // Subtle fills stay translucent — they are washes over the row /
    // control beneath, exactly as `SubtleFillColorSecondary` is used for
    // ListViewItem and MenuFlyoutItem hover in WinUI.
    c.surface_hover = p.subtle_fill_secondary;
    c.surface_pressed = p.subtle_fill_tertiary;
    // Win11 list selection is a neutral wash plus a leading accent pill
    // (drawn by `FluentStandardItemStyle`). The wash is taken one step
    // stronger than the `SubtleFillColorSecondary` WinUI uses, so views
    // that have no pill — the TableView selection band, GridView tiles —
    // still show a selection that is perceivable and not colour-alone
    // (WCAG 1.4.1 / 1.4.11).
    c.surface_selected = p.control_alt_fill_quarternary;
    c.surface_selected_inactive = p.subtle_fill_secondary;
    c.surface_alt_row = p.subtle_fill_tertiary;
    c.surface_disabled = p.control_fill_disabled;

    // ── Text ───────────────────────────────────────────────────────────
    c.text_primary = p.text_primary;
    c.text_secondary = p.text_secondary;
    c.text_disabled = p.text_disabled;
    c.text_on_accent = p.text_on_accent_primary;
    c.text_link = p.accent_text_primary;
    // WinUI's HyperlinkButton hover uses AccentTextFillColorSecondary,
    // which in the dark dictionary is the *same* resource as Primary — so
    // dark-theme links do not shift hue on hover, they rely on the
    // underline. Kept authentic rather than inventing a shift.
    c.text_link_hover = p.accent_text_secondary;
    // Derived: Fluent has no visited-link role. The tertiary accent text
    // is the de-emphasised member of the same family, which reads as
    // "already followed" without leaving the palette.
    c.text_link_visited = p.accent_text_tertiary;
    c.text_error = p.system_fill_critical;
    c.text_warning = p.system_fill_caution;
    c.text_success = p.system_fill_success;

    // ── Accent ─────────────────────────────────────────────────────────
    // Fluent's Secondary / Tertiary accent fills are the same brush at
    // 90 % / 80 % opacity; flattened here so an accent Button's hover and
    // pressed steps are predictable regardless of what is behind it.
    c.accent = p.accent_fill_default;
    c.accent_hover = over(p.accent_fill_secondary, base);
    c.accent_pressed = over(p.accent_fill_tertiary, base);
    c.accent_disabled = over(p.accent_fill_disabled, base);
    // Derived: Fluent's nearest token, `SystemFillColorAttentionBackground`,
    // is a *neutral* wash — it carries no accent hue at all, so it cannot
    // serve a role whose whole job is "subtle accent tint" (Badge fills,
    // info backgrounds). A low-alpha accent over the page background is
    // the smallest honest substitute; dark needs more alpha to read.
    c.accent_subtle_bg = over(
        p.accent_ramp
            .base
            .with_alpha(if light { 0.10 } else { 0.20 }),
        base,
    );

    // ── Borders / dividers ─────────────────────────────────────────────
    c.border = p.control_stroke_default;
    // `ControlStrongStrokeColorDefault` — the visible outline an unchecked
    // Checkbox / RadioButton and an off ToggleSwitch draw. Distinctly
    // heavier than `ControlStrokeColorSecondary` (which is the button's
    // elevation edge and is read from the palette by `FluentButtonStyle`).
    c.border_strong = p.control_strong_stroke_default;
    // Fluent's focus indicator is a high-contrast ring, NOT the accent —
    // black in light, white in dark. This is the most visible way the
    // preset departs from IntUI and Material 3.
    c.border_focused = p.focus_stroke_outer;
    c.border_error = p.system_fill_critical;
    c.border_warning = p.system_fill_caution;
    c.border_disabled = p.control_strong_stroke_disabled;
    c.divider = p.divider_stroke_default;
    c.divider_strong = p.control_stroke_secondary;

    // ── Status ─────────────────────────────────────────────────────────
    c.status_info_fg = p.system_fill_attention;
    c.status_info_bg = over(p.system_fill_attention_background, base);
    c.status_success_fg = p.system_fill_success;
    c.status_success_bg = over(p.system_fill_success_background, base);
    c.status_warning_fg = p.system_fill_caution;
    c.status_warning_bg = over(p.system_fill_caution_background, base);
    c.status_error_fg = p.system_fill_critical;
    c.status_error_bg = over(p.system_fill_critical_background, base);

    // ── Cross-design-language container / on-error roles ───────────────
    // The dark dictionary's critical colour is a pale pink, so the label
    // that sits on it has to flip to dark — resolved by contrast rather
    // than hard-coded per appearance.
    c.text_on_error = p.system_fill_critical.best_contrast_text();
    c.text_on_error_container = p.system_fill_critical;
    c.surface_error_container = over(p.system_fill_critical_background, base);
    c.surface_container = p.layer_base();
    c.surface_container_raised = p.solid_background_quarternary;
    c.surface_container_sunken = p.solid_background_secondary;

    // ── Selection ──────────────────────────────────────────────────────
    // WinUI's `TextControlSelectionHighlightColor` is the *base* accent in
    // both appearances (not the light/dark-shifted fill), so the selected
    // run's foreground is resolved against it by contrast.
    c.selection_bg_active = p.accent_ramp.base;
    c.selection_text_active = p.accent_ramp.base.best_contrast_text();
    c.selection_bg_inactive = over(p.control_alt_fill_quarternary, base);
    c.selection_text_inactive = p.text_primary;
    // `search_match_bg` / `search_match_current_bg` keep the IntUI amber —
    // Fluent specifies no find-highlight colour and amber reads on both
    // appearances.

    // ── Scrollbar ──────────────────────────────────────────────────────
    c.scrollbar_track = p.control_alt_fill_transparent;
    c.scrollbar_track_hover = p.control_alt_fill_secondary;
    c.scrollbar_thumb = p.control_strong_fill_default;
    // Derived: WinUI grows the thumb rather than recolouring it (Teksilo's
    // ScrollBar recipe animates thickness too), but a colour step on top
    // keeps the press readable when the bar is already expanded.
    let thumb_a = p.control_strong_fill_default.a();
    c.scrollbar_thumb_hover = p
        .control_strong_fill_default
        .with_alpha((thumb_a * 1.30).min(1.0));
    c.scrollbar_thumb_pressed = p
        .control_strong_fill_default
        .with_alpha((thumb_a * 1.55).min(1.0));

    // ── Tooltip ────────────────────────────────────────────────────────
    // Unlike IntUI (dark chip in both themes), a Fluent tooltip is a
    // flyout: the same surface, text and hairline as a menu.
    c.tooltip_bg = p.solid_background_tertiary;
    c.tooltip_text = p.text_primary;
    c.tooltip_border = p.surface_stroke_flyout;
    c.tooltip_shortcut = p.text_secondary;

    // ── Editor ─────────────────────────────────────────────────────────
    // The editor pane is not chrome: light goes to the brightest solid,
    // dark to `SolidBackgroundFillColorBaseAlt` (below the chrome), which
    // is the usual editor-versus-shell relationship on both appearances.
    let editor_bg = if light {
        p.solid_background_quarternary
    } else {
        p.solid_background_base_alt
    };
    c.editor_bg = editor_bg;
    c.editor_fg = p.text_primary;
    c.editor_caret = p.text_primary;
    // Derived: no Fluent token for a current-line band or a code block.
    c.editor_current_line_bg = over(p.subtle_fill_tertiary, editor_bg);
    c.editor_gutter_fg = p.text_tertiary;
    c.editor_selection_bg = over(
        p.accent_ramp
            .base
            .with_alpha(if light { 0.30 } else { 0.45 }),
        editor_bg,
    );
    c.editor_code_block_bg = over(
        if light {
            p.control_alt_fill_tertiary
        } else {
            p.control_alt_fill_quarternary
        },
        editor_bg,
    );
    // `text_primary` is 89 % black / pure white; code sits one notch past
    // it in the same direction so monospaced runs read as their own
    // register against prose.
    c.editor_code_block_fg = if light { Color::BLACK } else { Color::WHITE };

    // ── Misc ───────────────────────────────────────────────────────────
    c.focus_ring = p.focus_stroke_outer;
    c.focus_ring_error = p.system_fill_critical;
    c.scrim = p.smoke_fill_default;

    // `chart_palette` keeps the colourblind-safe Okabe-Ito default —
    // Fluent publishes no data-visualisation sequence.
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::presets::intui;
    use teksilo_tokens::{BorderRole, SurfaceRole};

    fn light() -> ColorTokens {
        fluent_light_colors(&FluentPalette::light())
    }

    fn dark() -> ColorTokens {
        fluent_dark_colors(&FluentPalette::dark())
    }

    /// Contrast of a possibly-translucent foreground against an opaque
    /// background. [`Color::contrast_ratio`] compares raw luminance and so
    /// ignores alpha — but almost every Fluent text and stroke token is an
    /// alpha-over-surface wash (`#E4000000` is 89 % black, not black), so
    /// the ratio is only meaningful after compositing.
    fn contrast_on(fg: Color, bg: Color) -> f32 {
        over(fg, bg).contrast_ratio(bg)
    }

    #[test]
    fn window_and_editor_surfaces_are_opaque() {
        for c in [light(), dark()] {
            for (name, col) in [
                ("surface_main", c.surface_main),
                ("surface_content", c.surface_content),
                ("surface_raised", c.surface_raised),
                ("surface_sunken", c.surface_sunken),
                ("editor_bg", c.editor_bg),
                ("accent", c.accent),
                ("accent_hover", c.accent_hover),
                ("accent_pressed", c.accent_pressed),
            ] {
                assert!((col.a() - 1.0).abs() < 1e-6, "{name} must be opaque");
            }
        }
    }

    /// A preset that starts from `ColorTokens::*_default()` silently
    /// inherits every token it forgets to map. For the disabled family
    /// that is not cosmetic — a Fluent **dark** app would paint a disabled
    /// field in IntUI's *light* grey. Mirrors the Material 3 guard.
    #[test]
    fn disabled_family_is_fluent_derived_not_the_intui_fallback() {
        for (name, f, base) in [
            ("light", light(), intui::light().colors),
            ("dark", dark(), intui::dark().colors),
        ] {
            assert_ne!(
                f.surface_disabled, base.surface_disabled,
                "{name}: surface_disabled still holds the IntUI fallback"
            );
            assert_ne!(
                f.border_disabled, base.border_disabled,
                "{name}: border_disabled still holds the IntUI fallback"
            );
            assert_ne!(
                f.text_disabled, base.text_disabled,
                "{name}: text_disabled still holds the IntUI fallback"
            );
        }
    }

    #[test]
    fn field_roles_follow_the_fluent_palette() {
        let c = dark();
        assert_eq!(SurfaceRole::Field.resolve(&c), c.surface_content);
        assert_eq!(SurfaceRole::Disabled.resolve(&c), c.surface_disabled);
        assert_eq!(BorderRole::Field.resolve(&c), c.border);
        assert_eq!(BorderRole::Disabled.resolve(&c), c.border_disabled);
    }

    #[test]
    fn focus_indicator_is_high_contrast_not_accent() {
        // The signature Fluent departure: the focus ring is near-black in
        // light and pure white in dark, never the accent.
        let l = light();
        let d = dark();
        assert_ne!(l.focus_ring, l.accent);
        assert_ne!(d.focus_ring, d.accent);
        assert!(l.focus_ring.relative_luminance() < 0.1);
        assert!(d.focus_ring.relative_luminance() > 0.9);
        assert_eq!(l.border_focused, l.focus_ring);
        assert_eq!(d.border_focused, d.focus_ring);
    }

    #[test]
    fn focus_ring_clears_the_non_text_contrast_floor() {
        // WCAG SC 1.4.11 — a focus indicator needs >= 3:1 against the
        // surface it is drawn on.
        for c in [light(), dark()] {
            assert!(
                contrast_on(c.focus_ring, c.surface_main) >= 3.0,
                "focus ring must clear 3:1 on the window surface"
            );
            assert!(
                contrast_on(c.focus_ring, c.surface_content) >= 3.0,
                "focus ring must clear 3:1 on a control surface"
            );
        }
    }

    #[test]
    fn body_text_clears_the_wcag_aa_floor() {
        for c in [light(), dark()] {
            for surface in [c.surface_main, c.surface_content, c.surface_raised] {
                assert!(
                    contrast_on(c.text_primary, surface) >= 4.5,
                    "primary text must clear 4.5:1"
                );
                assert!(
                    contrast_on(c.text_secondary, surface) >= 4.5,
                    "secondary text must clear 4.5:1"
                );
            }
        }
    }

    #[test]
    fn control_outlines_clear_the_non_text_contrast_floor() {
        // `border_strong` is the outline an unchecked Checkbox / off
        // ToggleSwitch draws — a UI component boundary, so WCAG 1.4.11
        // applies to it as much as to the focus ring.
        for c in [light(), dark()] {
            assert!(
                contrast_on(c.border_strong, c.surface_main) >= 3.0,
                "strong border must clear 3:1 on the window surface"
            );
        }
    }

    #[test]
    fn on_accent_text_clears_the_wcag_aa_floor() {
        // Fluent's asymmetric ramp exists precisely so this holds: light
        // fills with a darkened accent + white text, dark fills with a
        // lightened accent + black text.
        for c in [light(), dark()] {
            assert!(
                contrast_on(c.text_on_accent, c.accent) >= 4.5,
                "on-accent label must clear 4.5:1 against the accent fill"
            );
        }
    }

    #[test]
    fn tooltips_are_flyouts_not_inverse_chips() {
        // IntUI paints a dark chip in both themes; Fluent uses the flyout
        // surface, so the tooltip tracks the appearance.
        let l = light();
        let d = dark();
        assert!(l.tooltip_bg.relative_luminance() > 0.5);
        assert!(d.tooltip_bg.relative_luminance() < 0.5);
        assert!(contrast_on(l.tooltip_text, l.tooltip_bg) >= 4.5);
        assert!(contrast_on(d.tooltip_text, d.tooltip_bg) >= 4.5);
        assert!(contrast_on(l.tooltip_shortcut, l.tooltip_bg) >= 4.5);
        assert!(contrast_on(d.tooltip_shortcut, d.tooltip_bg) >= 4.5);
    }

    #[test]
    fn selection_is_stronger_than_hover_in_both_appearances() {
        // Both are neutral washes in Fluent; selection must still be the
        // heavier of the two or a pill-less view loses its selection cue.
        for c in [light(), dark()] {
            assert!(
                c.surface_selected.a() > c.surface_hover.a(),
                "selected wash must be heavier than the hover wash"
            );
        }
    }

    #[test]
    fn light_and_dark_have_distinct_surfaces() {
        assert_ne!(light().surface_main, dark().surface_main);
        assert_ne!(light().accent, dark().accent);
    }

    #[test]
    fn status_backgrounds_are_opaque_and_carry_their_foreground() {
        for c in [light(), dark()] {
            for (fg, bg) in [
                (c.status_error_fg, c.status_error_bg),
                (c.status_success_fg, c.status_success_bg),
                (c.status_warning_fg, c.status_warning_bg),
            ] {
                assert!((bg.a() - 1.0).abs() < 1e-6);
                assert!(contrast_on(fg, bg) >= 3.0);
            }
        }
    }
}
