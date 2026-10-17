// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The colour-scheme model: compiled defaults, the Header fallback, the
//! `[Inactive]` replacement, the stock Breeze constants and the text round
//! trip. The comparisons against the vendored breeze 6.8.0 files are in
//! `breeze_files.rs`, which is not published with the crate.

use teksilo_kde_scheme::{
    ColorEffect, ColorRole, ColorSet, ContrastEffect, IntensityEffect, KdeColor, KdeColorScheme,
    SetColors, StateEffect,
};

fn parse_clean(text: &str) -> KdeColorScheme {
    let (scheme, diagnostics) = KdeColorScheme::parse(text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    scheme
}

fn rgb(r: u8, g: u8, b: u8) -> KdeColor {
    KdeColor::rgb8(r, g, b)
}

// ---- The stock schemes -------------------------------------------------

#[test]
fn the_stock_schemes_carry_the_values_the_specification_lists() {
    let light = KdeColorScheme::breeze_light();
    assert_eq!(light.name.as_deref(), Some("Breeze Light"));
    assert_eq!(
        light.set(ColorSet::Window).background_normal,
        rgb(239, 240, 241)
    );
    assert_eq!(
        light.set(ColorSet::Selection).background_normal,
        rgb(61, 174, 233)
    );
    assert_eq!(
        light.set(ColorSet::View).decoration_hover,
        rgb(61, 174, 233)
    );
    assert_eq!(
        light
            .inactive_set(ColorSet::Header)
            .unwrap()
            .background_normal,
        rgb(239, 240, 241)
    );
    assert_eq!(light.contrast, 4);
    assert_eq!(light.frame_contrast, 0.2);
    assert_eq!(light.disabled_effect, StateEffect::DISABLED_DEFAULT);
    assert!(!light.inactive_effect.enabled);
    assert_eq!(light.inactive_effect.color_effect, ColorEffect::Fade);
    assert_eq!(light.inactive_effect.color_amount, 0.025);
    assert!(light.change_selection_color);
    assert_eq!(light.wm.active_background, Some(rgb(227, 229, 231)));

    let dark = KdeColorScheme::breeze_dark();
    assert_eq!(
        dark.set(ColorSet::Window).background_normal,
        rgb(32, 35, 38)
    );
    assert_eq!(dark.set(ColorSet::View).background_normal, rgb(20, 22, 24));
    assert_eq!(dark.wm.inactive_background, Some(rgb(32, 36, 40)));

    let classic = KdeColorScheme::breeze_classic();
    assert!(!classic.header_group_present);
    assert_eq!(classic.set(ColorSet::Header), classic.set(ColorSet::Window));
    assert!(classic.titlebar_is_accent_colored);
    assert_eq!(
        classic.set(ColorSet::Complementary).decoration_focus,
        rgb(30, 146, 255)
    );
}

#[test]
fn the_stock_schemes_and_the_defaults_round_trip_through_text() {
    for scheme in [
        KdeColorScheme::breeze_light(),
        KdeColorScheme::breeze_dark(),
        KdeColorScheme::breeze_classic(),
        KdeColorScheme::kcolorscheme_defaults(),
    ] {
        let text = scheme.to_kconfig_text();
        assert_eq!(parse_clean(&text), scheme, "round trip of:\n{text}");
    }
}

#[test]
fn a_stock_scheme_is_written_like_the_file_it_came_from() {
    let text = KdeColorScheme::breeze_dark().to_kconfig_text();
    assert!(
        text.starts_with("[ColorEffects:Disabled]\nColor=56,56,56\n"),
        "{text}"
    );
    assert!(text.contains("\n[Colors:Header][Inactive]\nBackgroundAlternate=41,44,48\n"));
    assert!(text.contains("\n[KDE]\ncontrast=4\n"));
    assert!(!text.contains("frameContrast"));
    assert!(text.ends_with("inactiveForeground=161,169,177\n"));
}

// ---- Defaults ----------------------------------------------------------

#[test]
fn an_empty_configuration_gives_the_compiled_defaults() {
    let scheme = parse_clean("");
    assert_eq!(scheme, KdeColorScheme::kcolorscheme_defaults());
    assert_eq!(scheme, KdeColorScheme::default());
    // The defaults equal Breeze Light except for the hover decoration.
    let light = KdeColorScheme::breeze_light();
    for set in ColorSet::ALL {
        let mut expected = *light.set(set);
        expected.decoration_hover = rgb(147, 206, 233);
        assert_eq!(*scheme.set(set), expected, "{set:?}");
    }
    assert_eq!(scheme.contrast, 7);
    assert_eq!(scheme.frame_contrast, 0.2);
    assert_eq!(scheme.disabled_effect, StateEffect::DISABLED_DEFAULT);
    assert_eq!(scheme.inactive_effect, StateEffect::INACTIVE_DEFAULT);
    // No [ColorEffects:Inactive] at all: effects off, selection change on.
    assert!(scheme.change_selection_color);
    assert_eq!(scheme.wm, Default::default());
    assert_eq!(scheme.accent, None);
    assert_eq!(scheme.tint_factor, None);
    assert!(!scheme.titlebar_is_accent_colored);
    assert!(scheme.inactive_sets.iter().all(Option::is_none));
}

#[test]
fn absent_and_unreadable_keys_take_the_default_of_their_own_set() {
    let scheme = parse_clean(
        "[Colors:Button]\nBackgroundNormal=1,2,3\nForegroundNormal=invalid\nForegroundLink=\nForegroundActive=300,0,0\n",
    );
    let button = scheme.set(ColorSet::Button);
    assert_eq!(button.background_normal, rgb(1, 2, 3));
    assert_eq!(button.background_alternate, rgb(163, 212, 250));
    assert_eq!(button.foreground_normal, rgb(35, 38, 41));
    assert_eq!(button.foreground_link, rgb(41, 128, 185));
    assert_eq!(button.foreground_active, rgb(61, 174, 233));
}

#[test]
fn contrast_defaults_to_seven_and_is_not_clamped() {
    assert_eq!(parse_clean("[KDE]\ncontrast=abc\n").contrast, 7);
    assert_eq!(parse_clean("[KDE]\ncontrast=15\n").contrast, 15);
    assert_eq!(parse_clean("[KDE]\ncontrast=-3\n").contrast, -3);
}

#[test]
fn frame_contrast_defaults_to_one_fifth_and_is_clamped_to_the_unit_interval() {
    assert_eq!(
        parse_clean("[KDE]\nframeContrast=junk\n").frame_contrast,
        0.2
    );
    assert_eq!(
        parse_clean("[KDE]\nframeContrast=0.35\n").frame_contrast,
        0.35
    );
    assert_eq!(parse_clean("[KDE]\nframeContrast=4\n").frame_contrast, 1.0);
    assert_eq!(parse_clean("[KDE]\nframeContrast=-1\n").frame_contrast, 0.0);
}

// ---- Header -----------------------------------------------------------

#[test]
fn without_a_header_group_the_header_set_is_the_window_group_key_by_key() {
    let scheme =
        parse_clean("[Colors:Window]\nBackgroundNormal=10,10,10\nForegroundNormal=20,20,20\n");
    assert!(!scheme.header_group_present);
    let header = scheme.set(ColorSet::Header);
    assert_eq!(header.background_normal, rgb(10, 10, 10));
    assert_eq!(header.foreground_normal, rgb(20, 20, 20));
    // Window lacks BackgroundAlternate, so the Header table applies, not
    // the Window table.
    assert_eq!(header.background_alternate, rgb(239, 240, 241));
    assert_eq!(
        scheme.set(ColorSet::Window).background_alternate,
        rgb(227, 229, 231)
    );
}

#[test]
fn a_partial_header_group_takes_its_other_keys_from_window() {
    let scheme = parse_clean(
        "[Colors:Window]\nBackgroundNormal=10,10,10\nDecorationHover=5,5,5\n[Colors:Header]\nForegroundNormal=1,1,1\n",
    );
    assert!(scheme.header_group_present);
    let header = scheme.set(ColorSet::Header);
    assert_eq!(header.foreground_normal, rgb(1, 1, 1));
    assert_eq!(header.background_normal, rgb(10, 10, 10));
    assert_eq!(header.decoration_hover, rgb(5, 5, 5));
    assert_eq!(header.decoration_focus, rgb(61, 174, 233));
    assert_eq!(header.background_alternate, rgb(239, 240, 241));
}

#[test]
fn a_header_without_its_own_group_keeps_its_table_fallback_through_text() {
    let scheme = parse_clean("[Colors:Window]\nBackgroundNormal=10,10,10\n");
    let text = scheme.to_kconfig_text();
    assert!(!text.contains("[Colors:Header]"), "{text}");
    assert_eq!(parse_clean(&text), scheme, "{text}");
}

// ---- [Inactive] --------------------------------------------------------

#[test]
fn an_inactive_subgroup_replaces_its_set_and_falls_back_to_the_defaults_not_the_parent() {
    let scheme = parse_clean(
        "[Colors:View]\nBackgroundNormal=1,1,1\nForegroundNormal=2,2,2\n[Colors:View][Inactive]\nBackgroundNormal=9,9,9\n",
    );
    let inactive = scheme.inactive_set(ColorSet::View).unwrap();
    assert_eq!(inactive.background_normal, rgb(9, 9, 9));
    // Not the parent's 2,2,2: the View table.
    assert_eq!(inactive.foreground_normal, rgb(35, 38, 41));
    assert_eq!(scheme.set(ColorSet::View).foreground_normal, rgb(2, 2, 2));
    assert_eq!(scheme.inactive_set(ColorSet::Window), None);
}

#[test]
fn a_header_inactive_subgroup_falls_back_to_window_then_the_header_table() {
    let scheme = parse_clean(
        "[Colors:Window]\nForegroundNormal=3,3,3\n[Colors:Header][Inactive]\nBackgroundNormal=9,9,9\n",
    );
    // A nested group alone makes the Header group exist.
    assert!(scheme.header_group_present);
    let inactive = scheme.inactive_set(ColorSet::Header).unwrap();
    assert_eq!(inactive.background_normal, rgb(9, 9, 9));
    assert_eq!(inactive.foreground_normal, rgb(3, 3, 3));
    assert_eq!(inactive.background_alternate, rgb(239, 240, 241));
}

#[test]
fn an_inactive_subgroup_with_only_deleted_keys_does_not_exist() {
    let scheme = parse_clean("[Colors:View][Inactive]\nBackgroundNormal[$d]\n");
    assert_eq!(scheme.inactive_set(ColorSet::View), None);
}

#[test]
fn schemes_with_inactive_subgroups_round_trip() {
    let scheme = parse_clean(
        "[Colors:Selection][Inactive]\nBackgroundNormal=1,2,3,128\n[Colors:Window][Inactive]\nForegroundNormal=#102030\n",
    );
    let text = scheme.to_kconfig_text();
    assert_eq!(parse_clean(&text), scheme, "{text}");
}

// ---- Effects ------------------------------------------------------------

#[test]
fn effects_read_every_key_and_unknown_codes_mean_none() {
    let scheme = parse_clean(
        "[ColorEffects:Disabled]\nEnable=false\nIntensityEffect=3\nIntensityAmount=0.5\nColorEffect=3\n\
         ColorAmount=0.4\nColor=10,20,30\nContrastEffect=9\nContrastAmount=0.3\n",
    );
    assert_eq!(
        scheme.disabled_effect,
        StateEffect {
            enabled: false,
            intensity_effect: IntensityEffect::Lighten,
            intensity_amount: 0.5,
            color_effect: ColorEffect::Tint,
            color_amount: 0.4,
            color: rgb(10, 20, 30),
            contrast_effect: ContrastEffect::None,
            contrast_amount: 0.3,
        }
    );
}

#[test]
fn the_effect_colour_is_read_only_when_a_colour_effect_is_set() {
    let none = parse_clean("[ColorEffects:Disabled]\nColorEffect=0\nColor=1,2,3\n");
    assert_eq!(none.disabled_effect.color, rgb(56, 56, 56));
    let unknown = parse_clean("[ColorEffects:Inactive]\nColorEffect=7\nColor=1,2,3\n");
    assert_eq!(unknown.inactive_effect.color_effect, ColorEffect::None);
    assert_eq!(unknown.inactive_effect.color, rgb(112, 111, 110));
}

#[test]
fn change_selection_color_defaults_to_the_inactive_enable_flag() {
    assert!(!parse_clean("[ColorEffects:Inactive]\nEnable=false\n").change_selection_color);
    assert!(parse_clean("[ColorEffects:Inactive]\nEnable=true\n").change_selection_color);
    assert!(
        parse_clean("[ColorEffects:Inactive]\nEnable=false\nChangeSelectionColor=true\n")
            .change_selection_color
    );
    // An empty value is present and reads as true.
    assert!(
        parse_clean("[ColorEffects:Inactive]\nEnable=\n")
            .inactive_effect
            .enabled
    );
}

// ---- [General] and [WM] -------------------------------------------------

#[test]
fn general_keys_follow_their_fallbacks() {
    let scheme =
        parse_clean("[General]\nTintFactor=oops\naccentActiveTitlebar=true\nAccentColor=#3daee9\n");
    assert_eq!(scheme.tint_factor, Some(0.15));
    assert!(scheme.titlebar_is_accent_colored);
    assert_eq!(scheme.accent, Some(rgb(61, 174, 233)));

    let newer = parse_clean(
        "[General]\nTitlebarIsAccentColored=false\naccentActiveTitlebar=true\nTintFactor=0.3\n",
    );
    assert!(!newer.titlebar_is_accent_colored);
    assert_eq!(newer.tint_factor, Some(0.3));
}

#[test]
fn every_written_field_round_trips() {
    let mut scheme = KdeColorScheme::breeze_dark();
    scheme.name = Some(" Odd \\ name\twith=signs ".to_owned());
    scheme.accent = Some(KdeColor::rgba8(1, 2, 3, 4));
    scheme.tint_factor = Some(0.125);
    scheme.titlebar_is_accent_colored = true;
    scheme.frame_contrast = 0.75;
    scheme.contrast = 7;
    scheme.wm.active_blend = None;
    scheme.change_selection_color = false;
    let mut set = SetColors::from_rgb8([[7, 7, 7]; 12]);
    set.set(ColorRole::DecorationHover, KdeColor::rgba8(9, 9, 9, 0));
    scheme.inactive_sets[ColorSet::Tooltip.index()] = Some(set);
    let text = scheme.to_kconfig_text();
    assert_eq!(parse_clean(&text), scheme, "{text}");
}

// ---- Source ---------------------------------------------------------------

#[test]
fn a_parsed_scheme_keeps_a_value_as_written_even_when_it_reads_it_clamped() {
    let raw = parse_clean("[KDE]\nframeContrast=4\n");
    assert_eq!(raw.frame_contrast, 1.0);
    assert_eq!(
        raw.source().unwrap().get(&["KDE"], "frameContrast"),
        Some("4")
    );
}
