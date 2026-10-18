// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The WCAG lifts of the Breeze colour mapping, and a sweep of every text
//! and backdrop pair the tokens declare.
//!
//! Each lift (D1 to D6 and D9 of the colour mapping's documentation) has a
//! premise test, that the scheme's own colour fails in stock Breeze, and a
//! test that the token passes; D10 and D11, which stock Breeze never needs,
//! have their premises in the schemes built to defeat the lifts. The sweep
//! then checks every declared pair in all four paint states (an active and
//! an inactive window, each with and without high contrast), over Breeze
//! Light, Dark and Classic, every scheme the golden vectors record (Oxygen
//! and Oxygen Dark turn inactive effects on, and two synthetic schemes
//! carry intensity effects and translucent colours), the ten Plasma accents
//! on Light and on Dark, and schemes built to defeat the lifts, each shown
//! to need them. A second sweep checks the faces the styles paint their own
//! text and marks on (a push button's in every state, a combo box's, a
//! table header section's, a check box's) over the same schemes and paint
//! states. The property at the end does both over random coherent schemes,
//! with random inactive effects.
//!
//! This file reads `tools/kde_golden/vectors/resolve.json` from the
//! repository, so it is left out of the published package (the `exclude`
//! list in `Cargo.toml`).
//!
//! The property builds four complete token sets per case, each lifting some
//! thirty colours by up to 256 luma steps against up to eleven backdrops:
//! about 5 ms a case in a debug build, so it keeps proptest's default 256
//! cases. Manual override knob: `PROPTEST_CASES=N cargo test -p
//! teksilo-theme-kde --test contrast`.
//!
//! Safe run (engineering/docs/property-testing.md):
//!
//! ```text
//! cargo test -p teksilo-theme-kde --test contrast --no-run
//! BIN=$(cargo test -p teksilo-theme-kde --test contrast --no-run --message-format=json 2>/dev/null \
//!   | jq -r 'select(.executable != null) | .executable' | tail -1)
//! ( ulimit -v 6000000 -t 300; "$BIN" --test-threads=1 )
//! ```

use proptest::prelude::*;
use serde_json::Value;
use teksilo_core::styles::{PaintState, Theme};
use teksilo_theme_kde::scheme::math::mix;
use teksilo_theme_kde::scheme::{
    ColorEffect, ColorSet, ColorState, ContrastEffect, DecorationRole, ForegroundRole,
    IntensityEffect, KdeColor, KdeColorScheme, PaletteRole, SetColors, StateEffect,
};
use teksilo_theme_kde::styles::checkbox::box_paint;
use teksilo_theme_kde::styles::chrome::{
    BreezeButtonColors, BreezeButtonKind, BreezeButtonState, button_paint,
};
use teksilo_theme_kde::styles::combo_box::combo_arrow_color;
use teksilo_theme_kde::styles::indicator::BreezeIndicatorColors;
use teksilo_theme_kde::styles::table::BreezeHeaderColors;
use teksilo_theme_kde::{self as kde, KdePalette, PlasmaAccent};
use teksilo_tokens::contrast::composite_contrast;
use teksilo_tokens::{Color, ColorTokens};

/// `[wcag]` SC 1.4.3 and SC 1.4.11.
const TEXT: f32 = 4.5;
const NON_TEXT: f32 = 3.0;

fn ratio(fg: Color, bg: Color) -> f32 {
    composite_contrast(fg, bg)
}

fn palette(theme: &Theme) -> &KdePalette {
    theme
        .extension::<KdePalette>()
        .expect("a KDE theme carries its palette")
}

fn role(theme: &Theme, r: PaletteRole) -> Color {
    palette(theme).role(ColorState::Active, r)
}

fn view_foreground(theme: &Theme, r: ForegroundRole) -> Color {
    palette(theme)
        .resolved()
        .set(ColorState::Active, ColorSet::View)
        .foreground(r)
        .to_color()
}

fn paint(theme: &Theme, window_active: bool, high_contrast: bool) -> ColorTokens {
    theme
        .for_paint_state(PaintState {
            window_active,
            high_contrast,
        })
        .colors
}

/// Every surface the tokens set text on, the hover and press washes
/// included.
fn text_surfaces(c: &ColorTokens) -> Vec<(&'static str, Color)> {
    vec![
        ("surface_main", c.surface_main),
        ("surface_content", c.surface_content),
        ("surface_raised", c.surface_raised),
        ("surface_container", c.surface_container),
        ("surface_container_raised", c.surface_container_raised),
        ("surface_container_sunken", c.surface_container_sunken),
        ("surface_sunken", c.surface_sunken),
        ("surface_alt_row", c.surface_alt_row),
        ("surface_hover", c.surface_hover),
        ("surface_pressed", c.surface_pressed),
        ("accent_subtle_bg", c.accent_subtle_bg),
    ]
}

/// Every declared pair of `c` that falls short of its floor, by name.
fn failures(c: &ColorTokens, text: f32, non_text: f32) -> Vec<String> {
    let mut pairs: Vec<(String, Color, Color, f32)> = Vec::new();
    let mut add = |fg_name: &str, fg: Color, bg_name: &str, bg: Color, floor: f32| {
        pairs.push((format!("{fg_name} on {bg_name}"), fg, bg, floor));
    };
    let surfaces = text_surfaces(c);
    for (name, fg) in [
        ("text_primary", c.text_primary),
        ("text_secondary", c.text_secondary),
        ("text_link", c.text_link),
        ("text_link_hover", c.text_link_hover),
        ("text_link_visited", c.text_link_visited),
        ("text_error", c.text_error),
        ("text_warning", c.text_warning),
        ("text_success", c.text_success),
        ("status_info_fg", c.status_info_fg),
    ] {
        for (bg_name, bg) in &surfaces {
            add(name, fg, bg_name, *bg, text);
        }
    }
    for (bg_name, bg) in [
        ("accent", c.accent),
        ("accent_hover", c.accent_hover),
        ("accent_pressed", c.accent_pressed),
    ] {
        add("text_on_accent", c.text_on_accent, bg_name, bg, text);
    }
    for (bg_name, bg) in [
        ("surface_selected", c.surface_selected),
        ("surface_selected_inactive", c.surface_selected_inactive),
        ("selection_bg_active", c.selection_bg_active),
        ("selection_bg_inactive", c.selection_bg_inactive),
    ] {
        add("text_primary", c.text_primary, bg_name, bg, text);
    }
    add(
        "selection_text_active",
        c.selection_text_active,
        "selection_bg_active",
        c.selection_bg_active,
        text,
    );
    add(
        "selection_text_inactive",
        c.selection_text_inactive,
        "selection_bg_inactive",
        c.selection_bg_inactive,
        text,
    );
    for (bg_name, bg) in [
        ("editor_bg", c.editor_bg),
        ("editor_current_line_bg", c.editor_current_line_bg),
        ("editor_selection_bg", c.editor_selection_bg),
        ("editor_code_block_bg", c.editor_code_block_bg),
    ] {
        add("editor_fg", c.editor_fg, bg_name, bg, text);
        add("editor_caret", c.editor_caret, bg_name, bg, text);
    }
    for (bg_name, bg) in [
        ("editor_bg", c.editor_bg),
        ("editor_current_line_bg", c.editor_current_line_bg),
    ] {
        add("editor_gutter_fg", c.editor_gutter_fg, bg_name, bg, text);
    }
    add(
        "editor_code_block_fg",
        c.editor_code_block_fg,
        "editor_code_block_bg",
        c.editor_code_block_bg,
        text,
    );
    add(
        "tooltip_text",
        c.tooltip_text,
        "tooltip_bg",
        c.tooltip_bg,
        text,
    );
    add(
        "tooltip_shortcut",
        c.tooltip_shortcut,
        "tooltip_bg",
        c.tooltip_bg,
        text,
    );
    for (name, fg, bg) in [
        ("status_info", c.status_info_fg, c.status_info_bg),
        ("status_success", c.status_success_fg, c.status_success_bg),
        ("status_warning", c.status_warning_fg, c.status_warning_bg),
        ("status_error", c.status_error_fg, c.status_error_bg),
    ] {
        add(name, fg, "its background", bg, text);
    }
    add(
        "text_on_error",
        c.text_on_error,
        "text_error",
        c.text_error,
        text,
    );
    add(
        "text_on_error_container",
        c.text_on_error_container,
        "surface_error_container",
        c.surface_error_container,
        text,
    );
    for (name, fg) in [
        ("focus_ring", c.focus_ring),
        ("border_focused", c.border_focused),
        ("border_strong", c.border_strong),
        ("divider_strong", c.divider_strong),
        ("border_error", c.border_error),
        ("border_warning", c.border_warning),
        ("focus_ring_error", c.focus_ring_error),
    ] {
        for (bg_name, bg) in [
            ("surface_main", c.surface_main),
            ("surface_content", c.surface_content),
            ("surface_raised", c.surface_raised),
        ] {
            add(name, fg, bg_name, bg, non_text);
        }
    }
    for (name, fg) in [
        ("scrollbar_thumb", c.scrollbar_thumb),
        ("scrollbar_thumb_hover", c.scrollbar_thumb_hover),
        ("scrollbar_thumb_pressed", c.scrollbar_thumb_pressed),
    ] {
        for (bg_name, bg) in [
            ("surface_main", c.surface_main),
            ("surface_content", c.surface_content),
        ] {
            add(name, fg, bg_name, bg, non_text);
        }
    }
    pairs
        .into_iter()
        .filter(|(_, fg, bg, floor)| ratio(*fg, *bg) < *floor)
        .map(|(name, fg, bg, floor)| {
            format!(
                "{name}: {:.2}:1 < {floor} ({fg:?} on {bg:?})",
                ratio(fg, bg)
            )
        })
        .collect()
}

// ── D1, text on the accent ──────────────────────────────────────────────

#[test]
fn d1_premise_breezes_selection_text_fails_on_its_highlight() {
    for t in [kde::light(), kde::dark()] {
        let text = role(&t, PaletteRole::HighlightedText);
        let fill = role(&t, PaletteRole::Highlight);
        assert!(ratio(text, fill) < TEXT, "{:.2}:1", ratio(text, fill));
    }
}

#[test]
fn d1_the_accent_text_is_plasmas_black_and_passes() {
    for t in [kde::light(), kde::dark()] {
        let c = &t.colors;
        assert_eq!(c.text_on_accent, Color::BLACK);
        for fill in [c.accent, c.accent_hover, c.accent_pressed] {
            assert!(ratio(c.text_on_accent, fill) >= TEXT);
        }
    }
    // The selected text: black on Breeze Light's highlight, and the
    // scheme's own text on Breeze Dark's darkened selection, where it
    // passes.
    assert_eq!(kde::light().colors.selection_text_active, Color::BLACK);
    let dark = kde::dark();
    assert_eq!(
        dark.colors.selection_text_active,
        role(&dark, PaletteRole::HighlightedText)
    );
    assert!(
        ratio(
            dark.colors.selection_text_active,
            dark.colors.selection_bg_active
        ) >= TEXT
    );
}

#[test]
fn d1_premise_breeze_darks_primary_text_fails_on_its_highlight() {
    let t = kde::dark();
    let fill = role(&t, PaletteRole::Highlight);
    assert!(ratio(t.colors.text_primary, fill) < TEXT);
}

#[test]
fn d1_the_selection_fill_carries_the_primary_text() {
    for t in [kde::light(), kde::dark()] {
        let c = &t.colors;
        assert!(ratio(c.text_primary, c.surface_selected) >= TEXT);
        assert!(ratio(c.editor_fg, c.editor_selection_bg) >= TEXT);
        // The fill keeps the highlight's hue.
        let (h1, _, _) = c.surface_selected.to_hsv();
        let (h2, _, _) = role(&t, PaletteRole::Highlight).to_hsv();
        assert!((h1 - h2).abs() < 6.0, "hue {h1} strays from {h2}");
    }
}

/// The fills D1 moves under the primary text: the hover and press washes
/// and the selections.
fn washes_and_selections(c: &ColorTokens) -> [(&'static str, Color); 7] {
    [
        ("surface_hover", c.surface_hover),
        ("surface_pressed", c.surface_pressed),
        ("accent_subtle_bg", c.accent_subtle_bg),
        ("surface_selected", c.surface_selected),
        ("surface_selected_inactive", c.surface_selected_inactive),
        ("selection_bg_active", c.selection_bg_active),
        ("editor_selection_bg", c.editor_selection_bg),
    ]
}

/// Breeze Light with its window and view text set to the grey `v`.
fn breeze_light_with_grey_text(v: u8) -> KdeColorScheme {
    with(
        KdeColorScheme::breeze_light(),
        &[ColorSet::View, ColorSet::Window],
        |set| set.foreground_normal = KdeColor::rgb8(v, v, v),
    )
}

#[test]
fn d1_a_wash_or_selection_stays_on_the_windows_side_of_a_mid_grey_text() {
    // An active light window whose text is in the middle of the scale, and
    // a backdrop window whose inactive effect shades the text toward it.
    let mut cases: Vec<(String, ColorTokens)> = (110..=130u8)
        .step_by(4)
        .map(|v| {
            let t = kde::from_scheme(&breeze_light_with_grey_text(v));
            (format!("grey {v}, active"), paint(&t, true, false))
        })
        .collect();
    for amount in [0.2, 0.25, 0.4] {
        let mut scheme = KdeColorScheme::breeze_light();
        scheme.inactive_effect = StateEffect {
            enabled: true,
            intensity_effect: IntensityEffect::Shade,
            intensity_amount: amount,
            ..StateEffect::INACTIVE_DEFAULT
        };
        let t = kde::from_scheme(&scheme);
        cases.push((format!("shade {amount}, inactive"), paint(&t, false, false)));
    }
    for (name, c) in cases {
        let text = c.text_primary.relative_luminance();
        let window = c.surface_main.relative_luminance();
        assert!(window > text, "{name}: the window is the lighter");
        for (fill_name, fill) in washes_and_selections(&c) {
            // Lighter than the text, as the window is: never moved across it
            // to black.
            assert!(
                fill.relative_luminance() > text,
                "{name}: {fill_name} {fill:?} crossed the text {:?}",
                c.text_primary
            );
            assert!(
                ratio(c.text_primary, fill) >= TEXT,
                "{name}: text on {fill_name}"
            );
        }
    }
}

// ── D10, the scheme's own text ──────────────────────────────────────────

#[test]
fn d10_a_scheme_text_that_passes_is_kept_and_one_that_fails_is_lifted() {
    for t in [kde::light(), kde::dark()] {
        for (_, active, high_contrast, _, _) in &PAINT_STATES[..2] {
            let c = paint(&t, *active, *high_contrast);
            let state = if *active {
                ColorState::Active
            } else {
                ColorState::Inactive
            };
            assert_eq!(
                c.text_primary,
                palette(&t).role(state, PaletteRole::WindowText)
            );
            assert_eq!(c.editor_fg, palette(&t).role(state, PaletteRole::Text));
        }
    }
    let t = kde::from_scheme(&breeze_light_with_grey_text(118));
    let vendor = role(&t, PaletteRole::WindowText);
    let c = &t.colors;
    assert!(ratio(vendor, c.surface_main) < TEXT);
    assert_ne!(c.text_primary, vendor);
    assert!(c.text_primary.relative_luminance() < vendor.relative_luminance());
    for (name, bg) in text_surfaces(c) {
        assert!(ratio(c.text_primary, bg) >= TEXT, "on {name}");
    }
    assert!(ratio(c.editor_fg, c.editor_bg) >= TEXT);
}

// ── D11, surfaces in the middle of the scale ────────────────────────────

#[test]
fn d11_an_inactive_window_shaded_toward_the_middle_is_moved_back() {
    // Breeze Dark with an inactive shade of 0.1: white reaches 6.44:1 on
    // the inactive window, short of the 7:1 high-contrast floor.
    let mut scheme = KdeColorScheme::breeze_dark();
    scheme.inactive_effect = StateEffect {
        enabled: true,
        intensity_effect: IntensityEffect::Shade,
        intensity_amount: 0.1,
        ..StateEffect::INACTIVE_DEFAULT
    };
    let t = kde::from_scheme(&scheme);
    let vendor = palette(&t).role(ColorState::Inactive, PaletteRole::Window);
    assert!(ratio(Color::WHITE, vendor) < 7.0);
    let c = paint(&t, false, true);
    assert!(c.surface_main.relative_luminance() < vendor.relative_luminance());
    assert!(ratio(c.text_primary, c.surface_main) >= 7.0);
    // The normal projection needs no move: white reaches 4.5:1 there.
    assert_eq!(paint(&t, false, false).surface_main, vendor);
    // Nor does any stock surface, in any paint state.
    for t in [kde::light(), kde::dark()] {
        for (_, active, high_contrast, _, _) in PAINT_STATES {
            let c = paint(&t, active, high_contrast);
            let state = if active {
                ColorState::Active
            } else {
                ColorState::Inactive
            };
            assert_eq!(c.surface_main, palette(&t).role(state, PaletteRole::Window));
            assert_eq!(
                c.surface_content,
                palette(&t).role(state, PaletteRole::Base)
            );
        }
    }
}

// ── D2, inactive text ───────────────────────────────────────────────────

#[test]
fn d2_premise_breeze_lights_inactive_text_fails_on_its_window() {
    let t = kde::light();
    let inactive = palette(&t)
        .resolved()
        .set(ColorState::Active, ColorSet::Window)
        .foreground(ForegroundRole::Inactive)
        .to_color();
    assert!(ratio(inactive, role(&t, PaletteRole::Window)) < TEXT);
}

#[test]
fn d2_secondary_text_passes_on_every_surface() {
    for t in [kde::light(), kde::dark()] {
        let c = &t.colors;
        for (name, bg) in text_surfaces(c) {
            assert!(
                ratio(c.text_secondary, bg) >= TEXT,
                "secondary text on {name}: {:.2}:1",
                ratio(c.text_secondary, bg)
            );
        }
        assert!(ratio(c.tooltip_shortcut, c.tooltip_bg) >= TEXT);
        assert!(ratio(c.editor_gutter_fg, c.editor_bg) >= TEXT);
    }
}

// ── D3, links ───────────────────────────────────────────────────────────

#[test]
fn d3_premise_breeze_lights_link_fails_on_its_window() {
    let t = kde::light();
    let link = view_foreground(&t, ForegroundRole::Link);
    assert!(ratio(link, role(&t, PaletteRole::Window)) < TEXT);
}

#[test]
fn d3_links_pass_on_every_surface() {
    for t in [kde::light(), kde::dark()] {
        let c = &t.colors;
        for (name, bg) in text_surfaces(c) {
            for (link, fg) in [
                ("text_link", c.text_link),
                ("text_link_hover", c.text_link_hover),
                ("text_link_visited", c.text_link_visited),
            ] {
                assert!(ratio(fg, bg) >= TEXT, "{link} on {name}");
            }
        }
    }
}

// ── D4, status colours ──────────────────────────────────────────────────

#[test]
fn d4_premise_breeze_lights_status_colours_fail_on_its_view() {
    let t = kde::light();
    let base = role(&t, PaletteRole::Base);
    for r in [
        ForegroundRole::Negative,
        ForegroundRole::Neutral,
        ForegroundRole::Positive,
    ] {
        assert!(ratio(view_foreground(&t, r), base) < TEXT, "{r:?}");
    }
}

#[test]
fn d4_status_colours_pass_as_text_and_as_borders() {
    for t in [kde::light(), kde::dark()] {
        let c = &t.colors;
        for fg in [
            c.text_error,
            c.text_warning,
            c.text_success,
            c.status_info_fg,
        ] {
            for (name, bg) in text_surfaces(c) {
                assert!(ratio(fg, bg) >= TEXT, "{fg:?} on {name}");
            }
        }
        for fg in [c.border_error, c.border_warning, c.focus_ring_error] {
            assert!(ratio(fg, c.surface_main) >= NON_TEXT);
            assert!(ratio(fg, c.surface_content) >= NON_TEXT);
        }
    }
}

// ── D5, the focus decoration ────────────────────────────────────────────

#[test]
fn d5_premise_breeze_lights_focus_colour_fails_on_its_view() {
    let t = kde::light();
    let focus = palette(&t)
        .resolved()
        .set(ColorState::Active, ColorSet::View)
        .decoration(DecorationRole::Focus)
        .to_color();
    assert!(ratio(focus, role(&t, PaletteRole::Base)) < NON_TEXT);
}

/// Breeze Light with a translucent negative and focus colour, and the
/// golden scheme with a translucent focus and a transparent positive.
fn translucent_schemes() -> Vec<(&'static str, KdeColorScheme)> {
    let half = with(KdeColorScheme::breeze_light(), &TEXT_SETS, |set| {
        set.foreground_negative = KdeColor::rgba8(218, 68, 83, 128);
        set.decoration_focus = KdeColor::rgba8(61, 174, 233, 128);
    });
    vec![
        ("half-translucent negative and focus", half),
        ("SyntheticValueForms", golden_scheme("SyntheticValueForms")),
    ]
}

#[test]
fn d5_the_focus_ring_passes_and_stays_opaque() {
    let mut themes = vec![("Breeze Light", kde::light()), ("Breeze Dark", kde::dark())];
    for (name, scheme) in translucent_schemes() {
        themes.push((name, kde::from_scheme(&scheme)));
    }
    for (name, t) in themes {
        for (state, active, high_contrast, _, non_text) in PAINT_STATES {
            let c = paint(&t, active, high_contrast);
            assert_eq!(c.focus_ring.a(), 1.0, "{name}, {state}");
            assert_eq!(c.focus_ring, c.border_focused);
            for bg in [c.surface_main, c.surface_content, c.surface_raised] {
                assert!(ratio(c.focus_ring, bg) >= non_text, "{name}, {state}");
            }
        }
    }
}

#[test]
fn every_lifted_token_is_opaque_even_from_a_translucent_scheme_colour() {
    for (name, scheme) in translucent_schemes() {
        let t = kde::from_scheme(&scheme);
        // The premise: the scheme has translucent colours to lift.
        let view = &scheme.sets[ColorSet::View.index()];
        assert!(view.decoration_focus.a < u16::MAX, "{name}");
        for (state, active, high_contrast, _, _) in PAINT_STATES {
            let c = paint(&t, active, high_contrast);
            for (token, color) in [
                ("text_primary", c.text_primary),
                ("text_secondary", c.text_secondary),
                ("text_link", c.text_link),
                ("text_link_hover", c.text_link_hover),
                ("text_link_visited", c.text_link_visited),
                ("text_error", c.text_error),
                ("text_warning", c.text_warning),
                ("text_success", c.text_success),
                ("status_info_fg", c.status_info_fg),
                ("text_on_accent", c.text_on_accent),
                ("selection_text_active", c.selection_text_active),
                ("selection_text_inactive", c.selection_text_inactive),
                ("editor_fg", c.editor_fg),
                ("editor_gutter_fg", c.editor_gutter_fg),
                ("tooltip_text", c.tooltip_text),
                ("tooltip_shortcut", c.tooltip_shortcut),
                ("border_strong", c.border_strong),
                ("border_error", c.border_error),
                ("border_warning", c.border_warning),
                ("focus_ring", c.focus_ring),
                ("focus_ring_error", c.focus_ring_error),
                ("scrollbar_thumb", c.scrollbar_thumb),
                ("scrollbar_thumb_hover", c.scrollbar_thumb_hover),
            ] {
                assert_eq!(color.a(), 1.0, "{name}, {state}: {token} {color:?}");
            }
        }
    }
}

// ── D6, frame outlines ──────────────────────────────────────────────────

#[test]
fn d6_premise_the_frame_outline_fails_on_its_window() {
    let t = kde::light();
    let outline = palette(&t).colors(ColorState::Active).frame_outline;
    assert!(ratio(outline, role(&t, PaletteRole::Window)) < NON_TEXT);
}

#[test]
fn d6_the_strong_outline_passes_and_the_plain_one_is_breezes() {
    for t in [kde::light(), kde::dark()] {
        let c = &t.colors;
        assert_eq!(
            c.border,
            palette(&t).colors(ColorState::Active).frame_outline
        );
        for bg in [c.surface_main, c.surface_content, c.surface_raised] {
            assert!(ratio(c.border_strong, bg) >= NON_TEXT);
            assert!(ratio(c.divider_strong, bg) >= NON_TEXT);
        }
    }
}

// ── D9, the scroll-bar thumb ────────────────────────────────────────────

#[test]
fn d9_premise_breezes_thumbs_fail_on_the_window() {
    let t = kde::light();
    let window = palette(&t)
        .resolved()
        .role(ColorState::Active, PaletteRole::Window);
    let text = palette(&t)
        .resolved()
        .role(ColorState::Active, PaletteRole::WindowText);
    let hover = palette(&t)
        .resolved()
        .set(ColorState::Active, ColorSet::View)
        .decoration(DecorationRole::Hover);
    let idle = mix(window, text, 0.5 * 0.7 * 0.5).to_color();
    let hovered = mix(window, hover, 0.5).to_color();
    assert!(ratio(idle, window.to_color()) < NON_TEXT);
    assert!(ratio(hovered, window.to_color()) < NON_TEXT);
}

#[test]
fn d9_the_thumbs_pass_and_no_track_is_painted() {
    for t in [kde::light(), kde::dark()] {
        let c = &t.colors;
        for fg in [
            c.scrollbar_thumb,
            c.scrollbar_thumb_hover,
            c.scrollbar_thumb_pressed,
        ] {
            assert!(ratio(fg, c.surface_main) >= NON_TEXT);
            assert!(ratio(fg, c.surface_content) >= NON_TEXT);
        }
        assert_eq!(c.scrollbar_track.a(), 0.0);
        assert_eq!(c.scrollbar_track_hover.a(), 0.0);
    }
}

// ── The sweep ───────────────────────────────────────────────────────────

/// `scheme` with `edit` applied to each of `sets`.
fn with(
    mut scheme: KdeColorScheme,
    sets: &[ColorSet],
    edit: impl Fn(&mut SetColors),
) -> KdeColorScheme {
    for set in sets {
        edit(&mut scheme.sets[set.index()]);
    }
    scheme
}

const TEXT_SETS: [ColorSet; 4] = [
    ColorSet::View,
    ColorSet::Window,
    ColorSet::Button,
    ColorSet::Tooltip,
];

/// Schemes built to defeat the lifts: secondary, link, status and focus
/// colours that nearly vanish into their backgrounds, frame contrast at
/// both ends, a highlight that is the window colour or a mid grey, inactive
/// effects turned on, with and without an intensity effect that shades the
/// inactive backgrounds toward the middle of the scale, a scheme tinted
/// toward its accent, a window text in the middle of the scale, and
/// translucent status and focus colours.
fn adversarial() -> Vec<(&'static str, KdeColorScheme)> {
    let rgb = KdeColor::rgb8;
    let faint_light = {
        let mut s = with(KdeColorScheme::breeze_light(), &TEXT_SETS, |set| {
            set.foreground_inactive = rgb(220, 222, 224);
            set.foreground_active = rgb(210, 230, 250);
            set.foreground_link = rgb(200, 220, 250);
            set.foreground_visited = rgb(225, 205, 235);
            set.foreground_negative = rgb(250, 200, 200);
            set.foreground_neutral = rgb(250, 230, 200);
            set.foreground_positive = rgb(200, 240, 210);
            set.decoration_focus = rgb(239, 240, 241);
            set.decoration_hover = rgb(239, 240, 241);
        });
        s.frame_contrast = 0.0;
        s
    };
    let faint_dark = {
        let mut s = with(KdeColorScheme::breeze_dark(), &TEXT_SETS, |set| {
            set.foreground_inactive = rgb(45, 47, 50);
            set.foreground_active = rgb(20, 50, 80);
            set.foreground_link = rgb(20, 40, 90);
            set.foreground_visited = rgb(60, 30, 70);
            set.foreground_negative = rgb(90, 20, 20);
            set.foreground_neutral = rgb(90, 50, 0);
            set.foreground_positive = rgb(10, 60, 30);
            set.decoration_focus = rgb(32, 35, 38);
            set.decoration_hover = rgb(32, 35, 38);
        });
        s.frame_contrast = 0.0;
        s
    };
    let window_highlight = {
        let mut s = with(
            KdeColorScheme::breeze_light(),
            &[ColorSet::Selection],
            |set| {
                set.background_normal = rgb(239, 240, 241);
                set.foreground_normal = rgb(255, 255, 255);
            },
        );
        s.frame_contrast = 1.0;
        s
    };
    let grey = |scheme: KdeColorScheme| {
        with(scheme, &[ColorSet::Selection], |set| {
            set.background_normal = rgb(128, 128, 128);
            set.foreground_normal = rgb(128, 128, 128);
        })
    };
    let effects_on = {
        let mut s = KdeColorScheme::breeze_light();
        s.inactive_effect = StateEffect {
            enabled: true,
            ..StateEffect::INACTIVE_DEFAULT
        };
        s
    };
    let effects_on_dark = {
        let mut s = KdeColorScheme::breeze_dark();
        s.inactive_effect = StateEffect {
            enabled: true,
            ..StateEffect::INACTIVE_DEFAULT
        };
        s
    };
    let tinted = {
        let mut s = KdeColorScheme::breeze_light();
        s.tint_factor = Some(0.4);
        s.with_accent(PlasmaAccent::Pink.kde_color())
    };
    let tinted_dark = {
        let mut s = KdeColorScheme::breeze_dark();
        s.tint_factor = Some(0.4);
        s.with_accent(PlasmaAccent::Yellow.kde_color())
    };
    // A window and view text in the middle of the scale: lifting a light
    // fill until it carries this text at 4.5:1 cannot succeed on the light
    // side, and once went all the way to black.
    let mid_grey_text = with(
        KdeColorScheme::breeze_light(),
        &[ColorSet::View, ColorSet::Window],
        |set| set.foreground_normal = rgb(118, 118, 118),
    );
    // `[ColorEffects:Inactive]` with Enable and one intensity effect, the
    // other keys at KDE's defaults.
    let intensity = |mut s: KdeColorScheme, intensity_effect, intensity_amount| {
        s.inactive_effect = StateEffect {
            enabled: true,
            intensity_effect,
            intensity_amount,
            ..StateEffect::INACTIVE_DEFAULT
        };
        s
    };
    // Translucent colours, which the `r,g,b,a` form writes.
    let translucent = with(KdeColorScheme::breeze_light(), &TEXT_SETS, |set| {
        set.foreground_negative = KdeColor::rgba8(218, 68, 83, 128);
        set.decoration_focus = KdeColor::rgba8(61, 174, 233, 128);
    });
    vec![
        ("faint light", faint_light),
        ("faint dark", faint_dark),
        ("highlight is the window", window_highlight),
        (
            "grey highlight, light",
            grey(KdeColorScheme::breeze_light()),
        ),
        ("grey highlight, dark", grey(KdeColorScheme::breeze_dark())),
        ("inactive effects on, light", effects_on),
        ("inactive effects on, dark", effects_on_dark),
        ("tinted pink", tinted),
        ("tinted yellow, dark", tinted_dark),
        ("mid-grey text, light", mid_grey_text),
        (
            "inactive shade 0.2, light",
            intensity(KdeColorScheme::breeze_light(), IntensityEffect::Shade, 0.2),
        ),
        (
            "inactive shade 0.4, light",
            intensity(KdeColorScheme::breeze_light(), IntensityEffect::Shade, 0.4),
        ),
        (
            "inactive shade 0.1, dark",
            intensity(KdeColorScheme::breeze_dark(), IntensityEffect::Shade, 0.1),
        ),
        (
            "inactive lighten 0.4, dark",
            intensity(KdeColorScheme::breeze_dark(), IntensityEffect::Lighten, 0.4),
        ),
        ("translucent status and focus", translucent),
    ]
}

const RESOLVE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/kde_golden/vectors/resolve.json"
));

/// An installed scheme the golden vectors record, parsed from its groups
/// written back as configuration text (a nested group is recorded with its
/// path joined by `][`, so bracketing the name gives the header line).
fn golden_scheme(wanted: &str) -> KdeColorScheme {
    let all: Value = serde_json::from_str(RESOLVE_JSON).expect("resolve.json parses");
    let scheme = all["schemes"]
        .as_array()
        .expect("resolve.json has a list of schemes")
        .iter()
        .find(|s| s["name"].as_str() == Some(wanted))
        .unwrap_or_else(|| panic!("resolve.json has no scheme {wanted}"));
    let mut text = String::new();
    for (group, entries) in scheme["input"].as_object().expect("an input object") {
        text.push('[');
        text.push_str(group);
        text.push_str("]\n");
        for (key, value) in entries.as_object().expect("a group object") {
            text.push_str(key);
            text.push('=');
            text.push_str(value.as_str().expect("a value is a string"));
            text.push('\n');
        }
        text.push('\n');
    }
    KdeColorScheme::parse(&text).0
}

/// The name of every scheme the golden vectors record.
fn golden_names() -> Vec<String> {
    let all: Value = serde_json::from_str(RESOLVE_JSON).expect("resolve.json parses");
    all["schemes"]
        .as_array()
        .expect("resolve.json has a list of schemes")
        .iter()
        .map(|s| s["name"].as_str().expect("a scheme has a name").to_owned())
        .collect()
}

#[test]
fn the_golden_vectors_record_thirteen_schemes_two_with_inactive_intensity_or_alpha() {
    let names = golden_names();
    assert_eq!(names.len(), 13, "{names:?}");
    // The two the sweep needs most: an intensity effect in the Inactive
    // state, and translucent and transparent colours.
    let effects = golden_scheme("SyntheticInactiveEffects").inactive_effect;
    assert!(effects.enabled);
    assert_eq!(effects.intensity_effect, IntensityEffect::Shade);
    let forms = golden_scheme("SyntheticValueForms");
    let view = &forms.sets[ColorSet::View.index()];
    assert!(view.decoration_focus.a < u16::MAX);
    assert_eq!(view.foreground_positive.a, 0);
}

/// Oxygen's three schemes, the installed schemes with inactive effects.
fn oxygen() -> Vec<(&'static str, KdeColorScheme)> {
    vec![
        ("Oxygen", golden_scheme("Oxygen")),
        ("Oxygen Cold", golden_scheme("OxygenCold")),
        ("Oxygen Dark", golden_scheme("OxygenDark")),
    ]
}

#[test]
fn oxygen_and_oxygen_dark_turn_inactive_effects_on() {
    let schemes = oxygen();
    let enabled: Vec<bool> = schemes
        .iter()
        .map(|(_, s)| s.inactive_effect.enabled)
        .collect();
    assert_eq!(enabled, [true, false, true]);
    assert!(!schemes[0].1.is_dark());
    assert!(schemes[2].1.is_dark());
}

fn sweep_schemes() -> Vec<(String, KdeColorScheme)> {
    let mut out = vec![
        ("Breeze Light".to_owned(), KdeColorScheme::breeze_light()),
        ("Breeze Dark".to_owned(), KdeColorScheme::breeze_dark()),
        (
            "Breeze Classic".to_owned(),
            KdeColorScheme::breeze_classic(),
        ),
    ];
    for name in golden_names() {
        out.push((format!("golden {name}"), golden_scheme(&name)));
    }
    for accent in PlasmaAccent::ALL {
        out.push((
            format!("Breeze Light, {accent}"),
            KdeColorScheme::breeze_light().with_accent(accent.kde_color()),
        ));
        out.push((
            format!("Breeze Dark, {accent}"),
            KdeColorScheme::breeze_dark().with_accent(accent.kde_color()),
        ));
    }
    for (name, scheme) in adversarial() {
        out.push((name.to_owned(), scheme));
    }
    out
}

/// The four paint states, named, with the floors each is held to.
const PAINT_STATES: [(&str, bool, bool, f32, f32); 4] = [
    ("active", true, false, TEXT, NON_TEXT),
    ("inactive", false, false, TEXT, NON_TEXT),
    ("active, high contrast", true, true, 7.0, 4.5),
    ("inactive, high contrast", false, true, 7.0, 4.5),
];

#[test]
fn every_declared_pair_passes_for_every_scheme_in_every_paint_state() {
    let mut all = Vec::new();
    for (name, scheme) in sweep_schemes() {
        let theme = kde::from_scheme(&scheme);
        for (state, active, high_contrast, text, non_text) in PAINT_STATES {
            for failure in failures(&paint(&theme, active, high_contrast), text, non_text) {
                all.push(format!("{name}, {state}: {failure}"));
            }
        }
    }
    assert!(
        all.is_empty(),
        "{} failures:\n{}",
        all.len(),
        all.join("\n")
    );
}

/// What the Breeze styles paint on a face of their own rather than on a
/// surface token: the push button's label on its face in every state, a
/// combo box's arrow on its face, a table header's label and sort arrow on
/// its section, and a check box's mark on its box. Each face is the style's, read from the theme as the
/// paint walker projects it, so the pairs follow what is painted.
fn face_failures(theme: &Theme, window_active: bool, high_contrast: bool) -> Vec<String> {
    let (text, non_text) = if high_contrast {
        (7.0, 4.5)
    } else {
        (TEXT, NON_TEXT)
    };
    let projected = theme.for_paint_state(PaintState {
        window_active,
        high_contrast,
    });
    let c = &projected.colors;
    let mut pairs: Vec<(String, Color, Color, f32)> = Vec::new();

    let button = BreezeButtonColors::resolve(&projected, window_active, high_contrast, false);
    for kind in [BreezeButtonKind::Push, BreezeButtonKind::Default] {
        for state in [
            BreezeButtonState::Rest,
            BreezeButtonState::Highlighted,
            BreezeButtonState::Pressed,
        ] {
            for checked in [false, true] {
                let face = button_paint(kind, state, checked, window_active, &button)
                    .face
                    .expect("a push or default button has a face");
                pairs.push((
                    format!("{kind:?} button label on its {state:?} face, checked {checked}"),
                    c.text_primary,
                    face,
                    text,
                ));
            }
        }
    }

    let header = BreezeHeaderColors::resolve(&projected, window_active, high_contrast);
    for (hovered, pressed) in [(false, false), (true, false), (false, true)] {
        let fill = header.fill(hovered, pressed);
        let what = format!("hovered {hovered}, pressed {pressed}");
        pairs.push((
            format!("header label on its section, {what}"),
            c.text_primary,
            fill,
            text,
        ));
        pairs.push((
            format!("header sort arrow on its section, {what}"),
            header.arrow,
            fill,
            non_text,
        ));
    }

    let arrow = combo_arrow_color(&projected, window_active, high_contrast, false, false);
    for (what, face) in [("at rest", button.button), ("open", button.pressed)] {
        pairs.push((
            format!("combo box arrow on its face {what}"),
            arrow,
            face,
            non_text,
        ));
    }

    let indicator = BreezeIndicatorColors::resolve(&projected, window_active, high_contrast, false);
    for pressed in [false, true] {
        let paint = box_paint(true, false, pressed, false, &indicator);
        let face = paint.composited_fill().expect("the box face is opaque");
        pairs.push((
            format!("check mark on its box, pressed {pressed}"),
            paint.mark,
            face,
            non_text,
        ));
    }

    pairs
        .into_iter()
        .filter(|(_, fg, bg, floor)| ratio(*fg, *bg) < *floor)
        .map(|(name, fg, bg, floor)| {
            format!(
                "{name}: {:.2}:1 < {floor} ({fg:?} on {bg:?})",
                ratio(fg, bg)
            )
        })
        .collect()
}

/// The faces the styles paint reach the same floors as the tokens, for
/// every scheme of the sweep in every paint state: a face is the surface
/// its text was lifted on, moved back by D11 where the scheme's inactive
/// effects shade it toward the middle of the scale.
#[test]
fn every_face_a_style_paints_carries_its_text_for_every_scheme_in_every_paint_state() {
    let mut all = Vec::new();
    for (name, scheme) in sweep_schemes() {
        let theme = kde::from_scheme(&scheme);
        for (state, active, high_contrast, _, _) in PAINT_STATES {
            for failure in face_failures(&theme, active, high_contrast) {
                all.push(format!("{name}, {state}: {failure}"));
            }
        }
    }
    assert!(
        all.is_empty(),
        "{} failures:\n{}",
        all.len(),
        all.join("\n")
    );
}

/// A colour pair a scheme itself gives, read from the palette the theme
/// keeps, with the floor its token is lifted to.
#[derive(Debug, Clone, Copy)]
enum Vendor {
    /// A view foreground on the view (Base): D2, D3 and D4.
    OnView(ForegroundRole),
    /// The view's focus decoration on the view, a non-text pair: D5.
    FocusOnView,
    /// The selection text on the selection: D1's text.
    SelectionText,
    /// The window text on the selection: D1's fill.
    WindowTextOnSelection,
    /// The window text on the window: D10, and D11 where no text could
    /// reach the floor on the window.
    WindowTextOnWindow,
    /// The same pair at the high-contrast text floor, 7:1.
    WindowTextOnWindowEnhanced,
}

fn window_text_on_window(p: &KdePalette, state: ColorState) -> f32 {
    ratio(
        p.role(state, PaletteRole::WindowText),
        p.role(state, PaletteRole::Window),
    )
}

impl Vendor {
    /// The pair's contrast in `state`, and its floor.
    fn contrast(self, p: &KdePalette, state: ColorState) -> (f32, f32) {
        let view = p.resolved().set(state, ColorSet::View);
        let base = p.role(state, PaletteRole::Base);
        let highlight = p.role(state, PaletteRole::Highlight);
        match self {
            Vendor::OnView(r) => (ratio(view.foreground(r).to_color(), base), TEXT),
            Vendor::FocusOnView => (
                ratio(view.decoration(DecorationRole::Focus).to_color(), base),
                NON_TEXT,
            ),
            Vendor::SelectionText => (
                ratio(p.role(state, PaletteRole::HighlightedText), highlight),
                TEXT,
            ),
            Vendor::WindowTextOnSelection => (
                ratio(p.role(state, PaletteRole::WindowText), highlight),
                TEXT,
            ),
            Vendor::WindowTextOnWindow => (window_text_on_window(p, state), TEXT),
            Vendor::WindowTextOnWindowEnhanced => (window_text_on_window(p, state), 7.0),
        }
    }
}

/// For each adversarial scheme, the state it targets and the vendor pairs
/// in that state that fall short, so its passing the sweep is the lifts'
/// doing.
fn what_each_adversarial_scheme_defeats() -> Vec<(&'static str, ColorState, Vec<Vendor>)> {
    use ColorState::{Active, Inactive};
    use ForegroundRole::{Inactive as Secondary, Link, Negative, Neutral};
    use Vendor::*;
    let faint = vec![
        OnView(Secondary),
        OnView(Link),
        OnView(Negative),
        FocusOnView,
    ];
    vec![
        ("faint light", Active, faint.clone()),
        ("faint dark", Active, faint),
        ("highlight is the window", Active, vec![SelectionText]),
        (
            "grey highlight, light",
            Active,
            vec![SelectionText, WindowTextOnSelection],
        ),
        (
            "grey highlight, dark",
            Active,
            vec![SelectionText, WindowTextOnSelection],
        ),
        (
            "inactive effects on, light",
            Inactive,
            vec![OnView(Negative), OnView(Neutral), FocusOnView],
        ),
        (
            "inactive effects on, dark",
            Inactive,
            vec![OnView(Negative)],
        ),
        (
            "tinted pink",
            Active,
            vec![OnView(Secondary), OnView(Neutral)],
        ),
        (
            "tinted yellow, dark",
            Active,
            vec![WindowTextOnSelection, OnView(Negative)],
        ),
        (
            "mid-grey text, light",
            Active,
            vec![WindowTextOnWindow, WindowTextOnSelection],
        ),
        (
            "inactive shade 0.2, light",
            Inactive,
            vec![WindowTextOnWindow],
        ),
        (
            "inactive shade 0.4, light",
            Inactive,
            vec![WindowTextOnWindow],
        ),
        (
            "inactive shade 0.1, dark",
            Inactive,
            vec![WindowTextOnWindowEnhanced],
        ),
        (
            "inactive lighten 0.4, dark",
            Inactive,
            vec![WindowTextOnWindow],
        ),
        (
            "translucent status and focus",
            Active,
            vec![OnView(Negative), FocusOnView],
        ),
    ]
}

#[test]
fn the_adversarial_schemes_do_need_the_lifts() {
    // A scheme meant to defeat the lifts that every pair passed without
    // them would test nothing: its own colours must fail.
    let schemes = adversarial();
    let needs = what_each_adversarial_scheme_defeats();
    let names: Vec<&str> = schemes.iter().map(|(n, _)| *n).collect();
    let listed: Vec<&str> = needs.iter().map(|(n, _, _)| *n).collect();
    assert_eq!(names, listed, "an adversarial scheme is unlisted");
    for ((name, scheme), (_, state, pairs)) in schemes.into_iter().zip(needs) {
        let theme = kde::from_scheme(&scheme);
        let p = palette(&theme);
        for pair in pairs {
            let (got, floor) = pair.contrast(p, state);
            assert!(got < floor, "{name}: {pair:?} already passes, {got:.2}:1");
            // An inactive-effects scheme earns its place by the effect: the
            // Inactive colour falls further short than the Active one.
            if state == ColorState::Inactive {
                let (active, _) = pair.contrast(p, ColorState::Active);
                assert!(
                    got < active,
                    "{name}: {pair:?} is no worse inactive ({got:.2}:1) than active ({active:.2}:1)"
                );
            }
        }
    }
}

// ── Random coherent schemes ─────────────────────────────────────────────

/// One 8-bit triple with every channel in `range`. Cost: three bytes.
fn arb_triple(range: std::ops::RangeInclusive<u8>) -> impl Strategy<Value = [u8; 3]> {
    [range.clone(), range.clone(), range]
}

/// A set of twelve colours: both backgrounds on the scheme's side, the
/// normal text on the other, the other nine free. Cost: twelve triples.
fn arb_set(dark: bool) -> impl Strategy<Value = SetColors> {
    let (side, text) = if dark {
        (0..=45, 200..=255)
    } else {
        (210..=255, 0..=60)
    };
    (
        arb_triple(side.clone()),
        arb_triple(side),
        arb_triple(text),
        prop::collection::vec(any::<[u8; 3]>(), 9),
    )
        .prop_map(|(bg, alt, fg, free)| {
            let mut rows = [[0u8; 3]; 12];
            rows[0] = bg;
            rows[1] = alt;
            rows[2] = fg;
            rows[3..].copy_from_slice(&free);
            SetColors::from_rgb8(rows)
        })
}

/// A random `[ColorEffects:Inactive]`: enabled or not, each of the three
/// effects any of its kinds at any amount in `0..=1`, and any colour.
///
/// Cost: one flag, three small enums, three numbers and a triple.
fn arb_inactive_effect() -> impl Strategy<Value = StateEffect> {
    (
        any::<bool>(),
        prop_oneof![
            Just(IntensityEffect::None),
            Just(IntensityEffect::Shade),
            Just(IntensityEffect::Darken),
            Just(IntensityEffect::Lighten),
        ],
        0.0_f64..=1.0,
        prop_oneof![
            Just(ColorEffect::None),
            Just(ColorEffect::Desaturate),
            Just(ColorEffect::Fade),
            Just(ColorEffect::Tint),
        ],
        0.0_f64..=1.0,
        any::<[u8; 3]>(),
        prop_oneof![
            Just(ContrastEffect::None),
            Just(ContrastEffect::Fade),
            Just(ContrastEffect::Tint),
        ],
        0.0_f64..=1.0,
    )
        .prop_map(
            |(
                enabled,
                intensity_effect,
                intensity_amount,
                color_effect,
                color_amount,
                [r, g, b],
                contrast_effect,
                contrast_amount,
            )| StateEffect {
                enabled,
                intensity_effect,
                intensity_amount,
                color_effect,
                color_amount,
                color: KdeColor::rgb8(r, g, b),
                contrast_effect,
                contrast_amount,
            },
        )
}

/// A coherent random scheme.
///
/// Coherent means its surfaces are on one side, every window, view, button
/// and tooltip background light (channels 210 to 255) or dark (0 to 45),
/// and its normal texts on the other (0 to 60, or 200 to 255). Every
/// colour a lift or a rule derives (inactive text, links, status colours,
/// decorations, the whole selection set) is free, as are the frame
/// contrast, the contrast and the inactive effects, which can shade an
/// inactive window anywhere on the scale.
///
/// Cost: four sets of twelve colours plus twelve free ones, two numbers, a
/// flag and an effect. Building the theme is the expensive part (see the
/// module doc); the scheme itself is about 60 triples.
fn arb_scheme() -> impl Strategy<Value = KdeColorScheme> {
    any::<bool>().prop_flat_map(|dark| {
        (
            arb_set(dark),
            arb_set(dark),
            arb_set(dark),
            arb_set(dark),
            prop::collection::vec(any::<[u8; 3]>(), 12),
            0.0_f64..=1.0,
            0_i32..=10,
            arb_inactive_effect(),
        )
            .prop_map(
                move |(view, window, button, tooltip, selection, fc, contrast, inactive)| {
                    let mut scheme = if dark {
                        KdeColorScheme::breeze_dark()
                    } else {
                        KdeColorScheme::breeze_light()
                    };
                    scheme.sets[ColorSet::View.index()] = view;
                    scheme.sets[ColorSet::Window.index()] = window;
                    scheme.sets[ColorSet::Button.index()] = button;
                    scheme.sets[ColorSet::Tooltip.index()] = tooltip;
                    let mut rows = [[0u8; 3]; 12];
                    rows.copy_from_slice(&selection);
                    scheme.sets[ColorSet::Selection.index()] = SetColors::from_rgb8(rows);
                    scheme.frame_contrast = fc;
                    scheme.contrast = contrast;
                    scheme.inactive_effect = inactive;
                    scheme
                },
            )
    })
}

// ── 1. every declared pair of every coherent scheme reaches its floor in
//      all four paint states: 4.5:1 text and 3:1 non-text for an active
//      and an inactive window, 7:1 and 4.5:1 under high contrast ──
proptest! {
    #[test]
    fn every_coherent_scheme_meets_its_floors_in_every_paint_state(scheme in arb_scheme()) {
        let theme = kde::from_scheme(&scheme);
        for (active, high_contrast) in [(true, false), (false, false), (true, true), (false, true)] {
            let (text, non_text) = if high_contrast { (7.0, 4.5) } else { (TEXT, NON_TEXT) };
            let mut f = failures(&paint(&theme, active, high_contrast), text, non_text);
            f.extend(face_failures(&theme, active, high_contrast));
            prop_assert!(
                f.is_empty(),
                "active {} high contrast {}: {:?}\nscheme {:?}",
                active, high_contrast, f, scheme.sets,
            );
        }
    }
}
