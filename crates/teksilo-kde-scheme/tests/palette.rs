// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! State effects, the resolved palette, the dark predicate and the colour
//! report, on the stock Breeze schemes and on schemes built to exercise one
//! rule each. The expected 8-bit values are the specification's worked
//! examples (sections e.4, f.1 and d.10); `golden_resolve.rs` checks every
//! colour against the measured vectors.

use teksilo_kde_scheme::effects::{
    SELECTION_TINT_AMOUNT, apply_state_effect, background_chain, foreground_chain,
    tint_toward_selection,
};
use teksilo_kde_scheme::math::{darken, lighten, mix, tint};
use teksilo_kde_scheme::{
    BackgroundRole, ColorEffect, ColorRole, ColorSet, ColorState, ContrastEffect, ForegroundRole,
    IntensityEffect, KdeColor, KdeColorScheme, PaletteRole, StateEffect,
};
use teksilo_tokens::ColorSchemePreference;

fn rgb(c: KdeColor) -> [u8; 3] {
    let [r, g, b, _] = c.to_rgba8();
    [r, g, b]
}

#[test]
fn breeze_disabled_darkens_a_fade_toward_the_background() {
    let scheme = KdeColorScheme::breeze_light();
    let disabled = scheme.state_colors(ColorState::Disabled, ColorSet::Window);
    assert_eq!(rgb(disabled.foreground_normal), [160, 161, 163]);
    assert_eq!(rgb(disabled.background_normal), [227, 229, 231]);

    // Every foreground and decoration is the chain darken(mix(X, BG0, 0.65),
    // 0.1) against the background as it was before the effect; every
    // background is darken(X, 0.1).
    let window = scheme.set(ColorSet::Window);
    let background = window.background_normal;
    for role in ColorRole::ALL {
        let stored = window.get(role);
        let expected = match role {
            ColorRole::BackgroundNormal | ColorRole::BackgroundAlternate => {
                darken(stored, 0.1, 1.0)
            }
            _ => darken(mix(stored, background, 0.65), 0.1, 1.0),
        };
        assert_eq!(disabled.get(role), expected, "{role:?}");
    }
}

#[test]
fn the_chains_apply_contrast_then_intensity_then_colour() {
    let effect = StateEffect {
        enabled: true,
        intensity_effect: IntensityEffect::Lighten,
        intensity_amount: 0.2,
        color_effect: ColorEffect::Tint,
        color_amount: 0.5,
        color: KdeColor::rgb8(56, 56, 56),
        contrast_effect: ContrastEffect::Tint,
        contrast_amount: 0.6,
    };
    let x = KdeColor::rgb8(200, 60, 40);
    let bg = KdeColor::rgb8(20, 30, 40);
    let lightened = lighten(x, 0.2, 1.0);
    assert_eq!(
        background_chain(x, &effect),
        tint(lightened, effect.color, 0.5)
    );
    let contrasted = tint(x, bg, 0.6);
    assert_eq!(
        foreground_chain(x, bg, &effect),
        background_chain(contrasted, &effect)
    );
}

#[test]
fn a_disabled_effect_passes_colours_through_without_requantising() {
    // A 16-bit colour that an HCY round trip would move.
    let odd = KdeColor::rgba16(12345, 23456, 34567, 65535);
    let mut effect = StateEffect::DISABLED_DEFAULT;
    effect.enabled = false;
    assert_eq!(background_chain(odd, &effect), odd);
    assert_eq!(foreground_chain(odd, KdeColor::WHITE, &effect), odd);
    let mut set = *KdeColorScheme::breeze_light().set(ColorSet::View);
    set.foreground_normal = odd;
    assert_eq!(apply_state_effect(&set, &effect), set);
}

#[test]
fn breeze_inactive_selection_is_the_window_tinted_toward_the_selection() {
    let scheme = KdeColorScheme::breeze_light();
    let inactive = scheme.state_colors(ColorState::Inactive, ColorSet::Selection);
    assert_eq!(rgb(inactive.background_normal), [194, 224, 245]);
    assert_eq!(rgb(inactive.background_alternate), [176, 216, 243]);
    assert_eq!(rgb(inactive.foreground_normal), [35, 38, 41]);
    assert_eq!(
        inactive,
        tint_toward_selection(
            scheme.set(ColorSet::Window),
            scheme.set(ColorSet::Selection).background_normal
        )
    );
    assert_eq!(SELECTION_TINT_AMOUNT, 0.4);

    let palette = scheme.resolve();
    assert_eq!(
        rgb(palette.role(ColorState::Inactive, PaletteRole::Highlight)),
        [194, 224, 245]
    );
    assert_eq!(
        rgb(palette.role(ColorState::Inactive, PaletteRole::HighlightedText)),
        [35, 38, 41]
    );
    assert_eq!(
        palette.role(ColorState::Inactive, PaletteRole::Accent),
        palette.role(ColorState::Inactive, PaletteRole::Highlight)
    );
}

#[test]
fn the_selection_tint_follows_the_inactive_window_subgroup() {
    let (scheme, _) = KdeColorScheme::parse(
        "[Colors:Window]\nBackgroundNormal=239,240,241\n\
         [Colors:Window][Inactive]\nBackgroundNormal=200,200,200\n\
         [Colors:Selection]\nBackgroundNormal=200,40,120\n",
    );
    let inactive = scheme.state_colors(ColorState::Inactive, ColorSet::Selection);
    let window = scheme.inactive_set(ColorSet::Window).expect("a subgroup");
    assert_eq!(
        inactive.background_normal,
        tint(window.background_normal, KdeColor::rgb8(200, 40, 120), 0.4)
    );
}

#[test]
fn without_the_selection_change_an_inactive_selection_keeps_its_colours() {
    let mut scheme = KdeColorScheme::breeze_light();
    scheme.change_selection_color = false;
    assert_eq!(
        scheme.state_colors(ColorState::Inactive, ColorSet::Selection),
        *scheme.set(ColorSet::Selection)
    );
}

#[test]
fn a_disabled_selection_is_the_disabled_window() {
    let scheme = KdeColorScheme::breeze_dark();
    assert_eq!(
        scheme.state_colors(ColorState::Disabled, ColorSet::Selection),
        scheme.state_colors(ColorState::Disabled, ColorSet::Window)
    );
}

#[test]
fn an_inactive_subgroup_replaces_the_set_and_the_effects_still_run() {
    let (scheme, _) = KdeColorScheme::parse(
        "[ColorEffects:Inactive]\nEnable=true\nIntensityEffect=2\nIntensityAmount=0.2\n\
         ColorEffect=0\nContrastEffect=0\n\
         [Colors:View][Inactive]\nBackgroundNormal=250,250,250\n",
    );
    let inactive = scheme.state_colors(ColorState::Inactive, ColorSet::View);
    let source = scheme.inactive_set(ColorSet::View).expect("a subgroup");
    assert_eq!(
        inactive.background_normal,
        darken(KdeColor::rgb8(250, 250, 250), 0.2, 1.0)
    );
    assert_eq!(
        inactive.foreground_normal,
        darken(source.foreground_normal, 0.2, 1.0)
    );
}

#[test]
fn breeze_inactive_colours_pass_through() {
    let scheme = KdeColorScheme::breeze_light();
    for set in ColorSet::ALL {
        if set == ColorSet::Selection {
            continue;
        }
        let expected = *scheme.inactive_set(set).unwrap_or_else(|| scheme.set(set));
        assert_eq!(
            scheme.state_colors(ColorState::Inactive, set),
            expected,
            "{set:?}"
        );
    }
}

#[test]
fn breeze_light_computed_backgrounds() {
    let palette = KdeColorScheme::breeze_light().resolve();
    let view = palette.set(ColorState::Active, ColorSet::View);
    assert_eq!(
        rgb(view.background(BackgroundRole::Active)),
        [236, 246, 252]
    );
    assert_eq!(
        rgb(view.background(BackgroundRole::Negative)),
        [250, 236, 237]
    );
    assert_eq!(
        rgb(view.background(BackgroundRole::Neutral)),
        [254, 241, 234]
    );
    assert_eq!(
        rgb(view.background(BackgroundRole::Positive)),
        [221, 251, 232]
    );
    // Each computed background tints Normal toward its foreground by 0.3.
    let normal = view.background(BackgroundRole::Normal);
    for role in BackgroundRole::ALL {
        if let Some(foreground) = role.tinted_toward() {
            assert_eq!(
                view.background(role),
                tint(normal, view.foreground(foreground), 0.3),
                "{role:?}"
            );
        }
    }
    assert_eq!(
        rgb(view.background(BackgroundRole::Alternate)),
        [247, 247, 247]
    );
    assert_eq!(rgb(view.foreground(ForegroundRole::Link)), [41, 128, 185]);
}

#[test]
fn computed_backgrounds_follow_the_effects() {
    let scheme = KdeColorScheme::breeze_light();
    let palette = scheme.resolve();
    let disabled = palette.set(ColorState::Disabled, ColorSet::View);
    let colors = scheme.state_colors(ColorState::Disabled, ColorSet::View);
    assert_eq!(
        disabled.background(BackgroundRole::Link),
        tint(colors.background_normal, colors.foreground_link, 0.3)
    );
}

#[test]
fn breeze_light_palette_roles() {
    let palette = KdeColorScheme::breeze_light().resolve();
    let active = |role| rgb(palette.role(ColorState::Active, role));
    assert_eq!(active(PaletteRole::Window), [239, 240, 241]);
    assert_eq!(active(PaletteRole::WindowText), [35, 38, 41]);
    assert_eq!(active(PaletteRole::Base), [255, 255, 255]);
    assert_eq!(active(PaletteRole::AlternateBase), [247, 247, 247]);
    assert_eq!(active(PaletteRole::PlaceholderText), [112, 125, 138]);
    assert_eq!(active(PaletteRole::Button), [252, 252, 252]);
    assert_eq!(active(PaletteRole::Highlight), [61, 174, 233]);
    assert_eq!(active(PaletteRole::HighlightedText), [255, 255, 255]);
    assert_eq!(active(PaletteRole::LinkVisited), [155, 89, 182]);
    // The shades of the window at contrast 4 (k = 0.4).
    assert_eq!(active(PaletteRole::Light), [255, 255, 255]);
    assert_eq!(active(PaletteRole::Midlight), [246, 247, 247]);
    assert_eq!(active(PaletteRole::Mid), [196, 200, 204]);
    assert_eq!(active(PaletteRole::Dark), [136, 142, 147]);
    assert_eq!(active(PaletteRole::Shadow), [71, 74, 76]);
    // Tooltips use the Active colours in every state.
    for state in ColorState::ALL {
        assert_eq!(
            rgb(palette.role(state, PaletteRole::TooltipBase)),
            [247, 247, 247]
        );
        assert_eq!(
            rgb(palette.role(state, PaletteRole::TooltipText)),
            [35, 38, 41]
        );
    }
    assert_eq!(
        rgb(palette.role(ColorState::Disabled, PaletteRole::WindowText)),
        [160, 161, 163]
    );
}

#[test]
fn the_contrast_value_is_a_tenth_of_the_contrast() {
    assert_eq!(KdeColorScheme::breeze_light().contrast_value(), 0.1 * 4.0);
    assert_eq!(
        KdeColorScheme::kcolorscheme_defaults().contrast_value(),
        0.1 * 7.0
    );
}

#[test]
fn breeze_light_and_classic_are_light_and_breeze_dark_is_dark() {
    assert!(!KdeColorScheme::breeze_light().is_dark());
    assert!(!KdeColorScheme::breeze_classic().is_dark());
    assert!(KdeColorScheme::breeze_dark().is_dark());
    assert!(!KdeColorScheme::kcolorscheme_defaults().is_dark());
}

#[test]
fn is_dark_is_the_grey_level_below_192() {
    let with_window = |r: u8, g: u8, b: u8| {
        KdeColorScheme::parse(&format!("[Colors:Window]\nBackgroundNormal={r},{g},{b}\n"))
            .0
            .is_dark()
    };
    // (11 r + 16 g + 5 b) / 32 with integer division.
    assert!(with_window(191, 191, 191));
    assert!(!with_window(192, 192, 192));
    // 11 x 200 + 16 x 190 + 5 x 180 = 6140, / 32 = 191.875 -> 191: dark,
    // although a rounded grey would be 192.
    assert!(with_window(200, 190, 180));
    // A dark window under light content views is dark.
    let (twilight, _) = KdeColorScheme::parse(
        "[Colors:Window]\nBackgroundNormal=49,54,59\n\
         [Colors:View]\nBackgroundNormal=252,252,252\n",
    );
    assert!(twilight.is_dark());
}

#[test]
fn breeze_light_colour_report() {
    let os = KdeColorScheme::breeze_light().os_theme_colors();
    let c = |r, g, b| Some(KdeColor::rgb8(r, g, b).to_color());
    assert_eq!(os.color_scheme, ColorSchemePreference::Light);
    assert_eq!(os.accent, c(61, 174, 233));
    assert_eq!(os.window_bg, c(239, 240, 241));
    assert_eq!(os.window_fg, c(35, 38, 41));
    assert_eq!(os.button_bg, c(252, 252, 252));
    assert_eq!(os.button_fg, c(35, 38, 41));
    assert_eq!(os.selection_bg, c(61, 174, 233));
    assert_eq!(os.selection_fg, c(255, 255, 255));
    assert_eq!(os.tooltip_bg, c(247, 247, 247));
    assert_eq!(os.tooltip_fg, c(35, 38, 41));
    assert_eq!(os.view_bg, c(255, 255, 255));
    assert_eq!(os.view_fg, c(35, 38, 41));
    assert_eq!(os.alternate_bg, c(247, 247, 247));
    assert_eq!(os.inactive_fg, c(112, 125, 138));
    assert_eq!(os.link, c(41, 128, 185));
    assert_eq!(os.link_visited, c(155, 89, 182));
    assert_eq!(os.error, c(218, 68, 83));
    assert_eq!(os.warning, c(246, 116, 0));
    assert_eq!(os.success, c(39, 174, 96));
    assert_eq!(os.focus, c(61, 174, 233));
}

#[test]
fn breeze_dark_colour_report() {
    let os = KdeColorScheme::breeze_dark().os_theme_colors();
    let c = |r, g, b| Some(KdeColor::rgb8(r, g, b).to_color());
    assert_eq!(os.color_scheme, ColorSchemePreference::Dark);
    assert_eq!(os.accent, c(61, 174, 233));
    assert_eq!(os.window_bg, c(32, 35, 38));
    assert_eq!(os.window_fg, c(252, 252, 252));
    assert_eq!(os.button_bg, c(41, 44, 48));
    assert_eq!(os.button_fg, c(252, 252, 252));
    assert_eq!(os.selection_bg, c(61, 174, 233));
    assert_eq!(os.selection_fg, c(252, 252, 252));
    assert_eq!(os.tooltip_bg, c(41, 44, 48));
    assert_eq!(os.tooltip_fg, c(252, 252, 252));
    assert_eq!(os.view_bg, c(20, 22, 24));
    assert_eq!(os.view_fg, c(252, 252, 252));
    assert_eq!(os.alternate_bg, c(29, 31, 34));
    assert_eq!(os.inactive_fg, c(161, 169, 177));
    assert_eq!(os.link, c(29, 153, 243));
    assert_eq!(os.link_visited, c(155, 89, 182));
    assert_eq!(os.error, c(218, 68, 83));
    assert_eq!(os.warning, c(246, 116, 0));
    assert_eq!(os.success, c(39, 174, 96));
    assert_eq!(os.focus, c(61, 174, 233));
}

#[test]
fn the_colour_report_reads_the_active_palette() {
    // The report and the resolved Active palette agree on every shared
    // colour, for a scheme whose sets all differ.
    let (scheme, _) = KdeColorScheme::parse(
        "[Colors:Window]\nBackgroundNormal=10,20,30\nForegroundNormal=200,201,202\n\
         [Colors:View]\nBackgroundNormal=240,241,242\nDecorationFocus=1,2,3\n\
         [Colors:Selection]\nBackgroundNormal=90,10,200\n",
    );
    let os = scheme.os_theme_colors();
    let palette = scheme.resolve();
    let role = |r| Some(palette.role(ColorState::Active, r).to_color());
    assert_eq!(os.color_scheme, ColorSchemePreference::Dark);
    assert_eq!(os.window_bg, role(PaletteRole::Window));
    assert_eq!(os.window_fg, role(PaletteRole::WindowText));
    assert_eq!(os.view_bg, role(PaletteRole::Base));
    assert_eq!(os.accent, role(PaletteRole::Accent));
    assert_eq!(os.selection_bg, role(PaletteRole::Highlight));
    assert_eq!(os.focus, Some(KdeColor::rgb8(1, 2, 3).to_color()));
}
