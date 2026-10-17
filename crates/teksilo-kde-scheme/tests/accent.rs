// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Accent application: the specification's worked examples (g.10), the ten
//! swatches through the selection rule (g.11), and the rules of section g
//! that the golden schemes do not exercise.

use teksilo_kde_scheme::accent::readable_link;
use teksilo_kde_scheme::math::{accent_over, black_or_white_on, luma, oklab_tint};
use teksilo_kde_scheme::{
    ACCENT_SWATCHES, ColorSet, ColorState, KConfig, KdeColor, KdeColorScheme,
};

fn rgb(r: u8, g: u8, b: u8) -> KdeColor {
    KdeColor::rgb8(r, g, b)
}

fn rgb8(c: KdeColor) -> [u8; 3] {
    let [r, g, b, _] = c.to_rgba8();
    [r, g, b]
}

fn parse_clean(text: &str) -> KdeColorScheme {
    let (scheme, diagnostics) = KdeColorScheme::parse(text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    scheme
}

fn written(scheme: &KdeColorScheme) -> &KConfig {
    scheme.source().expect("an accented scheme has a source")
}

const BREEZE_BLUE: KdeColor = KdeColor::rgb8(61, 174, 233);

// ---- Swatches (section h) -------------------------------------------------

#[test]
fn the_swatches_are_plasma_s_ten_in_order() {
    let hex: Vec<String> = ACCENT_SWATCHES
        .iter()
        .map(|c| {
            let [r, g, b] = rgb8(*c);
            format!("#{r:02x}{g:02x}{b:02x}")
        })
        .collect();
    assert_eq!(
        hex,
        [
            "#e93a9a", "#e93d58", "#e9643a", "#e8cb2d", "#3dd425", "#00d3b8", "#3daee9", "#b875dc",
            "#926ee4", "#686b6f",
        ]
    );
    assert!(ACCENT_SWATCHES.iter().all(|c| c.to_rgba8()[3] == 255));
}

#[test]
fn the_ten_swatches_give_the_specified_selection_through_with_accent() {
    // (light selection, black text, dark selection, black text), spec g.11.
    let table: [([u8; 3], bool, [u8; 3], bool); 10] = [
        ([239, 117, 184], true, [169, 47, 115], false),
        ([239, 119, 138], true, [169, 49, 68], false),
        ([239, 146, 117], true, [169, 76, 47], false),
        ([238, 218, 107], true, [168, 148, 38], true),
        ([119, 224, 102], true, [48, 155, 33], true),
        ([76, 224, 205], true, [5, 154, 136], true),
        ([119, 198, 239], true, [48, 128, 170], true),
        ([205, 158, 230], true, [134, 88, 161], false),
        ([178, 153, 236], true, [108, 83, 166], false),
        ([149, 151, 154], true, [78, 81, 84], false),
    ];
    let text = |black: bool| if black { [0, 0, 0] } else { [255, 255, 255] };
    for (swatch, (light, light_black, dark, dark_black)) in ACCENT_SWATCHES.into_iter().zip(table) {
        for (scheme, fill, black) in [
            (KdeColorScheme::breeze_light(), light, light_black),
            (KdeColorScheme::breeze_dark(), dark, dark_black),
        ] {
            let selection = *scheme.with_accent(swatch).set(ColorSet::Selection);
            assert_eq!(rgb8(selection.background_normal), fill, "swatch {swatch}");
            assert_eq!(
                rgb8(selection.background_alternate),
                fill,
                "swatch {swatch}"
            );
            assert_eq!(
                rgb8(selection.foreground_normal),
                text(black),
                "swatch {swatch}"
            );
            assert_eq!(
                rgb8(selection.foreground_inactive),
                text(black),
                "swatch {swatch}"
            );
        }
    }
}

// ---- Worked examples (section g.10) ---------------------------------------

#[test]
fn breeze_light_with_breeze_blue_matches_the_worked_example() {
    let accented = KdeColorScheme::breeze_light().with_accent(BREEZE_BLUE);
    let selection = accented.set(ColorSet::Selection);
    assert_eq!(selection.background_normal, rgb(119, 198, 239));
    assert_eq!(selection.background_alternate, rgb(119, 198, 239));
    assert_eq!(selection.foreground_normal, rgb(0, 0, 0));
    assert_eq!(selection.foreground_inactive, rgb(0, 0, 0));
    assert_eq!(selection.foreground_active, BREEZE_BLUE);
    assert_eq!(selection.decoration_focus, BREEZE_BLUE);
    assert_eq!(selection.decoration_hover, BREEZE_BLUE);

    for set in [
        ColorSet::View,
        ColorSet::Window,
        ColorSet::Button,
        ColorSet::Tooltip,
        ColorSet::Header,
        // Checked against its own 61,174,233: ratio 1, so darkened too.
        ColorSet::Selection,
    ] {
        assert_eq!(
            accented.set(set).foreground_link,
            rgb(45, 127, 170),
            "{set:?}"
        );
    }
    // 5.88 against 42,46,50: the accent itself.
    assert_eq!(
        accented.set(ColorSet::Complementary).foreground_link,
        BREEZE_BLUE
    );
    assert_eq!(
        accented.set(ColorSet::Button).background_alternate,
        rgb(118, 197, 238)
    );
}

#[test]
fn breeze_dark_with_breeze_blue_matches_the_worked_example() {
    let accented = KdeColorScheme::breeze_dark().with_accent(BREEZE_BLUE);
    let selection = accented.set(ColorSet::Selection);
    assert_eq!(selection.background_normal, rgb(48, 128, 170));
    assert_eq!(selection.foreground_normal, rgb(0, 0, 0));
    for set in [
        ColorSet::View,
        ColorSet::Window,
        ColorSet::Button,
        ColorSet::Tooltip,
        ColorSet::Header,
    ] {
        assert_eq!(accented.set(set).foreground_link, BREEZE_BLUE, "{set:?}");
    }
    assert_eq!(
        accented.set(ColorSet::Button).background_alternate,
        rgb(55, 135, 177)
    );
}

#[test]
fn breeze_light_with_its_own_accent_is_not_breeze_light() {
    let light = KdeColorScheme::breeze_light();
    let accented = light.with_accent(BREEZE_BLUE);
    assert_ne!(accented, light);
    assert_ne!(
        accented.set(ColorSet::Selection).background_normal,
        light.set(ColorSet::Selection).background_normal
    );
}

#[test]
fn the_link_is_made_readable_in_one_step() {
    assert_eq!(
        rgb8(readable_link(BREEZE_BLUE, rgb(255, 255, 255))),
        [45, 127, 170]
    );
    assert_eq!(readable_link(BREEZE_BLUE, rgb(20, 22, 24)), BREEZE_BLUE);
    // A light accent on a lighter-than-black but darker background is
    // lightened.
    let lightened = readable_link(rgb(120, 120, 200), rgb(90, 90, 90));
    assert!(luma(lightened) > luma(rgb(120, 120, 200)));
}

// ---- Rules of section g --------------------------------------------------

#[test]
fn applying_an_accent_twice_is_not_applying_it_once() {
    let accent = ACCENT_SWATCHES[0];
    let once = KdeColorScheme::breeze_light().with_accent(accent);
    let twice = once.with_accent(accent);
    assert_ne!(once, twice);
    // The first pass copies the stock Window colours into the Header
    // [Inactive] group; the second copies the accented ones.
    let inactive_focus =
        |s: &KdeColorScheme| s.inactive_set(ColorSet::Header).unwrap().decoration_focus;
    assert_eq!(inactive_focus(&once), BREEZE_BLUE);
    assert_eq!(inactive_focus(&twice), accent);
    // Applying to the original scheme again gives the same result.
    assert_eq!(KdeColorScheme::breeze_light().with_accent(accent), once);
}

#[test]
fn a_transparent_accent_copies_the_scheme() {
    let light = KdeColorScheme::breeze_light();
    let same = light.with_accent(KdeColor::TRANSPARENT);
    assert_eq!(same, light);
    let out = written(&same);
    assert_eq!(
        out.get(&["Colors:Selection"], "BackgroundNormal"),
        Some("61,174,233")
    );
    // The effect groups are still written, with empty values where the
    // scheme has no key.
    assert_eq!(out.get(&["ColorEffects:Disabled"], "Enable"), Some(""));
}

#[test]
fn a_transparent_accent_still_tints_inactive_subgroups_when_tinting_is_on() {
    let scheme = parse_clean(
        "[General]\nTintFactor=0.5\n[Colors:View]\nBackgroundNormal=200,40,40\n\
         [Colors:View][Inactive]\nBackgroundNormal=200,40,40\n",
    );
    let out = scheme.with_accent(KdeColor::TRANSPARENT);
    let out = written(&out);
    // Not tinted outside [Inactive]: there is no accent.
    assert_eq!(
        out.get(&["Colors:View"], "BackgroundNormal"),
        Some("200,40,40")
    );
    let tinted = oklab_tint(rgb(200, 40, 40), KdeColor::TRANSPARENT, 0.5).to_kconfig_text();
    assert_ne!(tinted, "200,40,40");
    assert_eq!(
        out.get(&["Colors:View", "Inactive"], "BackgroundNormal"),
        Some(tinted.as_str())
    );
}

#[test]
fn copied_keys_keep_their_text_and_missing_ones_stay_missing() {
    let scheme = parse_clean(
        "[Colors:View]\nBackgroundNormal=#ffffff\nForegroundNormal=#232629\n[Colors:Window]\nForegroundVisited=#9b59b6\n",
    );
    let out = scheme.with_accent(BREEZE_BLUE);
    let out = written(&out);
    assert_eq!(
        out.get(&["Colors:View"], "ForegroundNormal"),
        Some("#232629")
    );
    assert_eq!(out.get(&["Colors:View"], "BackgroundAlternate"), None);
    assert_eq!(
        out.get(&["Colors:Window"], "ForegroundVisited"),
        Some("#9b59b6")
    );
    // The accent keys are written in every group, missing or not.
    assert_eq!(
        out.get(&["Colors:Tooltip"], "DecorationHover"),
        Some("61,174,233")
    );
    // A group without BackgroundNormal checks the link against black.
    assert_eq!(
        out.get(&["Colors:Tooltip"], "ForegroundLink"),
        Some(
            readable_link(BREEZE_BLUE, KdeColor::BLACK)
                .to_kconfig_text()
                .as_str()
        )
    );
    // No Header group in the scheme: none written.
    assert!(!out.group_exists(&["Colors:Header"]));
}

#[test]
fn tinting_skips_keys_the_scheme_lacks() {
    let scheme = parse_clean(
        "[General]\nTintFactor=0.3\n[Colors:View]\nBackgroundNormal=255,255,255\nForegroundNormal=35,38,41\n",
    );
    let accent = ACCENT_SWATCHES[4];
    let out = scheme.with_accent(accent);
    let out = written(&out);
    let tinted = |c: KdeColor| oklab_tint(c, accent, 0.3).to_kconfig_text();
    assert_eq!(
        out.get(&["Colors:View"], "BackgroundNormal"),
        Some(tinted(rgb(255, 255, 255)).as_str())
    );
    assert_eq!(
        out.get(&["Colors:View"], "ForegroundNormal"),
        Some(tinted(rgb(35, 38, 41)).as_str())
    );
    assert_eq!(out.get(&["Colors:View"], "BackgroundAlternate"), None);
    assert_eq!(out.get(&["Colors:Window"], "BackgroundNormal"), None);
    // The result carries no tint factor.
    assert!(!out.has_key(&["General"], "TintFactor"));
}

#[test]
fn breeze_classic_has_no_header_and_an_accent_title_bar() {
    let classic = KdeColorScheme::breeze_classic();
    let accent = ACCENT_SWATCHES[1];
    let accented = classic.with_accent(accent);
    let out = written(&accented);
    assert!(!out.group_exists(&["Colors:Header"]));
    assert!(!accented.header_group_present);
    // The accent title bar without tinting: only the active title bar.
    let window = rgb(239, 240, 241);
    let fill = accent_over(accent, window);
    assert_eq!(accented.wm.active_background, Some(fill));
    assert_eq!(
        accented.wm.active_foreground,
        Some(black_or_white_on(fill, true))
    );
    assert_eq!(accented.wm.inactive_background, Some(rgb(239, 240, 241)));
    assert_eq!(accented.wm.active_blend, Some(rgb(252, 252, 252)));
    assert!(!accented.titlebar_is_accent_colored);
}

#[test]
fn a_scheme_without_wm_colours_gets_its_resolved_header_colours() {
    let scheme = parse_clean(
        "[Colors:Window]\nBackgroundNormal=200,201,202\nForegroundNormal=1,2,3\n\
         [ColorEffects:Inactive]\nEnable=true\nIntensityEffect=2\nIntensityAmount=0.5\n",
    );
    let accented = scheme.with_accent(BREEZE_BLUE);
    let active = scheme.state_colors(ColorState::Active, ColorSet::Header);
    let inactive = scheme.state_colors(ColorState::Inactive, ColorSet::Header);
    assert_eq!(active.background_normal, rgb(200, 201, 202));
    assert_ne!(inactive.background_normal, active.background_normal);
    // Written as text, so compared in 8 bits.
    let wm = accented.wm;
    let eight = |c: Option<KdeColor>| c.map(KdeColor::to_rgba8);
    let want = |c: KdeColor| Some(c.to_rgba8());
    assert_eq!(eight(wm.active_background), want(active.background_normal));
    assert_eq!(eight(wm.active_foreground), want(active.foreground_normal));
    assert_eq!(
        eight(wm.inactive_background),
        want(inactive.background_normal)
    );
    assert_eq!(
        eight(wm.inactive_foreground),
        want(inactive.foreground_normal)
    );
    assert_eq!(eight(wm.active_blend), want(active.background_normal));
    assert_eq!(eight(wm.inactive_blend), want(inactive.background_normal));
}

#[test]
fn tinting_without_an_accent_title_bar_tints_both_title_bars() {
    let scheme = parse_clean(
        "[General]\nTintFactor=0.2\n[Colors:Window]\nBackgroundNormal=239,240,241\n[WM]\nactiveBackground=1,1,1\n",
    );
    let accent = ACCENT_SWATCHES[2];
    let wm = scheme.with_accent(accent).wm;
    // Written as text, so compared in 8 bits.
    let eight = |c: Option<KdeColor>| c.map(KdeColor::to_rgba8);
    let tinted = oklab_tint(rgb(239, 240, 241), accent, 0.2);
    let want = |c: KdeColor| Some(c.to_rgba8());
    assert_eq!(eight(wm.active_background), want(tinted));
    assert_eq!(
        eight(wm.active_foreground),
        want(black_or_white_on(tinted, true))
    );
    assert_eq!(eight(wm.inactive_background), want(tinted));
    assert_eq!(
        eight(wm.inactive_foreground),
        want(black_or_white_on(tinted, false))
    );
}

#[test]
fn the_kde_group_is_written_as_read_or_with_its_defaults() {
    let out = parse_clean("").with_accent(BREEZE_BLUE);
    let out = written(&out);
    assert_eq!(out.get(&["KDE"], "contrast"), Some("7"));
    assert_eq!(out.get(&["KDE"], "frameContrast"), Some("0.2"));

    let raw = parse_clean("[KDE]\ncontrast=3\nframeContrast=1.5\n");
    let out = raw.with_accent(BREEZE_BLUE);
    assert_eq!(written(&out).get(&["KDE"], "contrast"), Some("3"));
    // Not clamped when written; clamped when read back.
    assert_eq!(written(&out).get(&["KDE"], "frameContrast"), Some("1.5"));
    assert_eq!(out.frame_contrast, 1.0);
}

#[test]
fn every_effect_key_is_written_and_empty_ones_read_as_enabled() {
    let accented = parse_clean("").with_accent(BREEZE_BLUE);
    let out = written(&accented);
    for group in ["ColorEffects:Inactive", "ColorEffects:Disabled"] {
        assert_eq!(out.keys(&[group]).len(), 9, "{group}");
        assert_eq!(out.get(&[group], "Enable"), Some(""), "{group}");
    }
    // A scheme with no Inactive effects group gets them switched on, as
    // KDE's empty Enable= reads.
    assert!(accented.inactive_effect.enabled);
    assert!(accented.disabled_effect.enabled);
}

#[test]
fn the_accented_scheme_keeps_its_name() {
    let accented = KdeColorScheme::breeze_dark().with_accent(BREEZE_BLUE);
    assert_eq!(accented.name.as_deref(), Some("Breeze Dark"));
    assert_eq!(accented.scheme_id.as_deref(), Some("BreezeDark"));
    assert_eq!(accented.accent, None);
}

#[test]
fn a_teksilo_color_is_accepted() {
    let from_tokens = KdeColorScheme::breeze_light().with_accent(BREEZE_BLUE.to_color());
    assert_eq!(
        from_tokens,
        KdeColorScheme::breeze_light().with_accent(BREEZE_BLUE)
    );
}
