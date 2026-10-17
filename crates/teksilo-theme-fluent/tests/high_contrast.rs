// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fluent light and dark under the OS "increase contrast" preference.
//!
//! The preset registers no `PaintProjection`, so it is painted through
//! `ColorTokens::for_high_contrast`, which lifts its own accent, ring,
//! secondary text and selection. Every pair that projection documents is
//! measured on both appearances, with the default accent and four others, for an active and an inactive window.

use teksilo_core::styles::{PaintState, Theme};
use teksilo_theme_fluent as fluent;
use teksilo_tokens::contrast::composite_contrast;
use teksilo_tokens::{Color, ColorTokens};

/// The default accent and four of Windows' accent swatches.
const ACCENTS: [&str; 4] = ["#B146C2", "#E74856", "#00B294", "#FFB900"];

fn themes() -> Vec<(String, Theme)> {
    let mut out = vec![
        ("Fluent light".to_owned(), fluent::light()),
        ("Fluent dark".to_owned(), fluent::dark()),
    ];
    for hex in ACCENTS {
        let accent = Color::from_hex(hex);
        out.push((
            format!("Fluent light, {hex}"),
            fluent::light_with_accent(accent),
        ));
        out.push((
            format!("Fluent dark, {hex}"),
            fluent::dark_with_accent(accent),
        ));
    }
    out
}

/// `[wcag]` SC 1.4.6 for text, and the 4.5:1 the projection holds every
/// non-text pair it changes to; 1.5:1 is how far it moves the selection off
/// `surface_content`.
const TEXT: f32 = 7.0;
const NON_TEXT: f32 = 4.5;
const SELECTION: f32 = 1.5;

fn high_contrast(theme: &Theme, window_active: bool) -> ColorTokens {
    theme
        .for_paint_state(PaintState {
            window_active,
            high_contrast: true,
        })
        .colors
}

/// Every pair `ColorTokens::for_high_contrast` documents, below its floor in
/// `c`, named with its ratio.
fn shortfalls(c: &ColorTokens) -> Vec<String> {
    let surfaces = [
        ("surface_main", c.surface_main),
        ("surface_content", c.surface_content),
        ("surface_raised", c.surface_raised),
    ];
    let mut pairs: Vec<(String, Color, Color, f32)> = Vec::new();
    for (fill, bg) in [
        ("accent", c.accent),
        ("accent_hover", c.accent_hover),
        ("accent_pressed", c.accent_pressed),
    ] {
        pairs.push((
            format!("text_on_accent on {fill}"),
            c.text_on_accent,
            bg,
            TEXT,
        ));
        // White text in a light palette, black in a dark one: the accent
        // clears the surfaces as well.
        let text_light = c.text_on_accent.relative_luminance() > 0.5;
        let surfaces_light = c.surface_main.relative_luminance() > 0.5;
        if text_light == surfaces_light {
            for (s, s_bg) in surfaces {
                pairs.push((format!("{fill} on {s}"), bg, s_bg, NON_TEXT));
            }
        }
    }
    for (s, bg) in surfaces {
        pairs.push((format!("focus_ring on {s}"), c.focus_ring, bg, NON_TEXT));
        pairs.push((
            format!("border_focused on {s}"),
            c.border_focused,
            bg,
            NON_TEXT,
        ));
        pairs.push((format!("text_secondary on {s}"), c.text_secondary, bg, TEXT));
    }
    // The focused selection, and the one a row keeps while its view is
    // unfocused or its window inactive.
    for (fill, bg) in [
        ("surface_selected", c.surface_selected),
        ("surface_selected_inactive", c.surface_selected_inactive),
    ] {
        pairs.push((
            format!("text_secondary on {fill}"),
            c.text_secondary,
            bg,
            TEXT,
        ));
        pairs.push((format!("text_primary on {fill}"), c.text_primary, bg, TEXT));
        pairs.push((
            format!("{fill} on surface_content"),
            bg,
            c.surface_content,
            SELECTION,
        ));
    }
    pairs
        .into_iter()
        .filter_map(|(name, fg, bg, floor)| {
            let r = composite_contrast(fg, bg);
            (r < floor).then(|| format!("{name} = {r:.2}, floor {floor}"))
        })
        .collect()
}

#[test]
fn every_high_contrast_pair_meets_its_floor() {
    for (name, theme) in themes() {
        for window_active in [true, false] {
            let short = shortfalls(&high_contrast(&theme, window_active));
            assert!(
                short.is_empty(),
                "{name}, window active {window_active}: {short:#?}"
            );
        }
    }
}

#[test]
fn high_contrast_keeps_the_accent_hue() {
    // The projection used to swap IntUI's teal into every dark palette.
    for (name, theme) in themes() {
        for window_active in [true, false] {
            let before = theme
                .for_paint_state(PaintState {
                    window_active,
                    high_contrast: false,
                })
                .colors
                .accent;
            let after = high_contrast(&theme, window_active).accent;
            let (h0, s0, _) = before.to_hsv();
            let (h1, s1, _) = after.to_hsv();
            if s0 > 0.05 && s1 > 0.05 {
                let d = (h0 - h1).abs();
                let d = d.min(360.0 - d);
                assert!(
                    d < 1.0,
                    "{name}, window active {window_active}: hue {h0} became {h1}"
                );
            }
        }
    }
}
