// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze preset's own high-contrast projection.
//!
//! Under the OS "increase contrast" preference every text must reach 7:1 on
//! what it is set on (WCAG SC 1.4.6) and every boundary, focus ring and the
//! accent 4.5:1, for Breeze Light and Dark, each of Plasma's ten accents on
//! both, and an active and an inactive window.
//!
//! The selection fill is the one fill held to its text only. It carries the
//! window's text at 7:1, and the window's text is at 7:1 on the window, so
//! no fill can also reach 4.5:1 on the window: the last tests check that
//! premise and that the fill keeps its text.

use teksilo_core::styles::{PaintState, Theme};
use teksilo_theme_kde::{self as kde, PlasmaAccent};
use teksilo_tokens::contrast::composite_contrast;
use teksilo_tokens::{Color, ColorTokens};

/// `[wcag]` SC 1.4.6 for text, and Teksilo's 4.5:1 for everything else.
const TEXT: f32 = 7.0;
const NON_TEXT: f32 = 4.5;

fn high_contrast(theme: &Theme, window_active: bool) -> ColorTokens {
    theme
        .for_paint_state(PaintState {
            window_active,
            high_contrast: true,
        })
        .colors
}

fn themes() -> Vec<(String, Theme)> {
    let mut out = vec![
        ("Breeze Light".to_owned(), kde::light()),
        ("Breeze Dark".to_owned(), kde::dark()),
    ];
    for accent in PlasmaAccent::ALL {
        out.push((
            format!("Breeze Light, {accent}"),
            kde::light_with_accent(accent.color()),
        ));
        out.push((
            format!("Breeze Dark, {accent}"),
            kde::dark_with_accent(accent.color()),
        ));
    }
    out
}

/// Every pair of `c` below its high-contrast floor.
fn failures(c: &ColorTokens) -> Vec<String> {
    let surfaces = [
        ("surface_main", c.surface_main),
        ("surface_content", c.surface_content),
        ("surface_raised", c.surface_raised),
        ("surface_container", c.surface_container),
        ("surface_container_raised", c.surface_container_raised),
        ("surface_sunken", c.surface_sunken),
        ("surface_alt_row", c.surface_alt_row),
        ("surface_hover", c.surface_hover),
        ("surface_pressed", c.surface_pressed),
    ];
    let mut pairs: Vec<(String, Color, Color, f32)> = Vec::new();
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
        for (bg_name, bg) in surfaces {
            pairs.push((format!("{name} on {bg_name}"), fg, bg, TEXT));
        }
    }
    for (bg_name, bg) in [
        ("accent", c.accent),
        ("accent_hover", c.accent_hover),
        ("accent_pressed", c.accent_pressed),
    ] {
        pairs.push((
            format!("text_on_accent on {bg_name}"),
            c.text_on_accent,
            bg,
            TEXT,
        ));
    }
    for (name, fg, bg) in [
        (
            "text_primary on surface_selected",
            c.text_primary,
            c.surface_selected,
        ),
        (
            "selection_text_active on selection_bg_active",
            c.selection_text_active,
            c.selection_bg_active,
        ),
        (
            "selection_text_inactive on selection_bg_inactive",
            c.selection_text_inactive,
            c.selection_bg_inactive,
        ),
        ("editor_fg on editor_bg", c.editor_fg, c.editor_bg),
        (
            "editor_fg on editor_selection_bg",
            c.editor_fg,
            c.editor_selection_bg,
        ),
        (
            "editor_gutter_fg on editor_bg",
            c.editor_gutter_fg,
            c.editor_bg,
        ),
        ("tooltip_text on tooltip_bg", c.tooltip_text, c.tooltip_bg),
        (
            "tooltip_shortcut on tooltip_bg",
            c.tooltip_shortcut,
            c.tooltip_bg,
        ),
        (
            "status_error_fg on status_error_bg",
            c.status_error_fg,
            c.status_error_bg,
        ),
        ("text_on_error on text_error", c.text_on_error, c.text_error),
    ] {
        pairs.push((name.to_owned(), fg, bg, TEXT));
    }
    for (name, fg) in [
        ("accent", c.accent),
        ("focus_ring", c.focus_ring),
        ("border_focused", c.border_focused),
        ("border_strong", c.border_strong),
        ("divider_strong", c.divider_strong),
        ("border_error", c.border_error),
        ("border_warning", c.border_warning),
    ] {
        for (bg_name, bg) in [
            ("surface_main", c.surface_main),
            ("surface_content", c.surface_content),
            ("surface_raised", c.surface_raised),
        ] {
            pairs.push((format!("{name} on {bg_name}"), fg, bg, NON_TEXT));
        }
    }
    for (name, fg) in [
        ("scrollbar_thumb", c.scrollbar_thumb),
        ("scrollbar_thumb_hover", c.scrollbar_thumb_hover),
    ] {
        for (bg_name, bg) in [
            ("surface_main", c.surface_main),
            ("surface_content", c.surface_content),
        ] {
            pairs.push((format!("{name} on {bg_name}"), fg, bg, NON_TEXT));
        }
    }
    pairs
        .into_iter()
        .filter(|(_, fg, bg, floor)| composite_contrast(*fg, *bg) < *floor)
        .map(|(name, fg, bg, floor)| {
            format!("{name}: {:.2}:1 < {floor}", composite_contrast(fg, bg))
        })
        .collect()
}

#[test]
fn every_swatch_meets_the_enhanced_floors_in_both_window_states() {
    let mut all = Vec::new();
    for (name, theme) in themes() {
        for (state, active) in [("active", true), ("inactive", false)] {
            for failure in failures(&high_contrast(&theme, active)) {
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

#[test]
fn secondary_text_is_the_primary_text() {
    for (name, theme) in themes() {
        for active in [true, false] {
            let c = high_contrast(&theme, active);
            assert_eq!(c.text_secondary, c.text_primary, "{name}");
            assert_eq!(c.tooltip_shortcut, c.tooltip_text, "{name}");
            assert_eq!(c.editor_gutter_fg, c.editor_fg, "{name}");
        }
    }
}

#[test]
fn the_frame_outline_is_the_text_at_full_frame_contrast() {
    for theme in [kde::light(), kde::dark()] {
        let c = high_contrast(&theme, true);
        assert_eq!(c.border, c.text_primary);
        assert_eq!(c.divider, c.text_primary);
    }
}

/// The documented exception to Plasma's black text on the accent: in a
/// light scheme the accent darkens and its text is white.
#[test]
fn a_light_scheme_puts_white_text_on_a_darkened_accent() {
    let normal = kde::light();
    let c = high_contrast(&normal, true);
    assert_eq!(normal.colors.text_on_accent, Color::BLACK);
    assert_eq!(c.text_on_accent, Color::WHITE);
    assert!(c.accent.relative_luminance() < normal.colors.accent.relative_luminance());
    let dark = high_contrast(&kde::dark(), true);
    assert_eq!(dark.text_on_accent, Color::BLACK);
}

/// The premise of that exception: no colour lets black text reach 7:1 on
/// it while it reaches 4.5:1 on Breeze Light's window.
#[test]
fn no_accent_carries_black_text_and_clears_a_light_window() {
    let window = kde::light().colors.surface_main;
    let mut found = None;
    for v in 0..=255u8 {
        let grey = Color::new(
            f32::from(v) / 255.0,
            f32::from(v) / 255.0,
            f32::from(v) / 255.0,
            1.0,
        );
        if composite_contrast(Color::BLACK, grey) >= TEXT
            && composite_contrast(grey, window) >= NON_TEXT
        {
            found = Some(v);
        }
    }
    // Luminance decides both ratios, and a grey reaches every luminance.
    assert_eq!(found, None);
}

#[test]
fn the_high_contrast_tokens_differ_from_the_normal_ones() {
    for theme in [kde::light(), kde::dark()] {
        assert_ne!(high_contrast(&theme, true), theme.colors);
        assert_ne!(high_contrast(&theme, false), high_contrast(&theme, true));
    }
}

/// The contrast of two relative luminances.
fn luminance_ratio(a: f32, b: f32) -> f32 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The premise of the high-contrast selection's limitation: for every
/// swatch and window state, no luminance a fill could have puts the
/// primary text at 7:1 on it and the fill at 4.5:1 on the window. Both
/// ratios depend on luminance alone, so a sweep of it covers every colour.
#[test]
fn no_selection_fill_carries_the_text_and_clears_the_window() {
    for (name, theme) in themes() {
        for active in [true, false] {
            let c = high_contrast(&theme, active);
            let text = c.text_primary.relative_luminance();
            let window = c.surface_main.relative_luminance();
            assert!(luminance_ratio(text, window) >= TEXT, "{name}");
            let found = (0..=100_000u32)
                .map(|i| i as f32 / 100_000.0)
                .find(|&fill| {
                    luminance_ratio(text, fill) >= TEXT && luminance_ratio(fill, window) >= NON_TEXT
                });
            assert_eq!(found, None, "{name}, active {active}");
        }
    }
}

/// What the projection does instead: the selection fill keeps the primary
/// and the editor text at 7:1, and stays below 4.5:1 on the window, as the
/// premise above forces.
#[test]
fn the_selection_fill_is_held_to_its_text() {
    for (name, theme) in themes() {
        for active in [true, false] {
            let c = high_contrast(&theme, active);
            for fill in [
                c.surface_selected,
                c.selection_bg_active,
                c.editor_selection_bg,
            ] {
                assert!(composite_contrast(c.text_primary, fill) >= TEXT, "{name}");
                assert!(composite_contrast(c.editor_fg, fill) >= TEXT, "{name}");
                assert!(
                    composite_contrast(fill, c.surface_main) < NON_TEXT,
                    "{name}"
                );
            }
        }
    }
    // The figures the colour mapping's documentation gives.
    for (theme, normal, high) in [(kde::light(), 2.18, 1.90), (kde::dark(), 3.40, 2.19)] {
        let n = &theme.colors;
        let h = high_contrast(&theme, true);
        let normal_ratio = composite_contrast(n.surface_selected, n.surface_main);
        let high_ratio = composite_contrast(h.surface_selected, h.surface_main);
        assert!((normal_ratio - normal).abs() < 0.01, "{normal_ratio}");
        assert!((high_ratio - high).abs() < 0.01, "{high_ratio}");
    }
}
