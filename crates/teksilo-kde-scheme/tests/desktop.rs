// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Desktop settings built from already-read layers.

use teksilo_kde_scheme::{
    BreezeSettings, ColorSet, KConfig, KdeColor, KdeColorScheme, KdeConfigLocations, KdeDesktop,
    KdeDesktopLayers, KdeFont, KdeFonts, LayerKind, MnemonicsMode, SchemeOrigin, ScrollBarButtons,
    ShadowSize,
};

fn no_env(_: &str) -> Option<String> {
    None
}

fn desktop(kdeglobals: &str, breezerc: &str) -> KdeDesktop {
    desktop_with_env(kdeglobals, breezerc, &no_env)
}

fn desktop_with_env(
    kdeglobals: &str,
    breezerc: &str,
    var: &dyn Fn(&str) -> Option<String>,
) -> KdeDesktop {
    let locations = KdeConfigLocations::from_env(|name: &str| match name {
        "HOME" => Some("/home/ada".to_owned()),
        _ => None,
    });
    let expansion = locations.expansion_env(var);
    let globals = [KConfig::parse(kdeglobals).0.with_kind(LayerKind::User)];
    let breeze = [KConfig::parse(breezerc).0.with_kind(LayerKind::User)];
    KdeDesktop::from_layers(
        &KdeDesktopLayers {
            kdeglobals: &globals,
            breezerc: &breeze,
            scheme_file: None,
        },
        &expansion,
    )
}

#[test]
fn an_empty_session_gives_breeze_light_and_every_default() {
    let d = desktop("", "");
    assert_eq!(d.scheme_origin, SchemeOrigin::BuiltIn);
    assert_eq!(d.scheme, KdeColorScheme::breeze_light());
    assert_eq!(d.user_accent, None);
    assert_eq!(d.fonts, KdeFonts::default());
    assert_eq!(d.animation_duration_factor, None);
    assert_eq!(d.widget_style, None);
    assert_eq!(d.breeze, BreezeSettings::default());
    assert!(d.uses_breeze_style());
    assert!(!d.prefers_reduced_motion());

    let mut expected = KdeDesktop::new(KdeColorScheme::breeze_light());
    expected.scheme_origin = SchemeOrigin::BuiltIn;
    assert_eq!(d, expected);
}

#[test]
fn the_default_fonts_are_noto_sans_and_hack() {
    let fonts = KdeFonts::default();
    assert_eq!(fonts.general.family, "Noto Sans");
    assert_eq!(fonts.general.point_size, Some(10.0));
    assert_eq!(fonts.small.point_size, Some(8.0));
    assert_eq!(fonts.fixed.family, "Hack");
    assert_eq!(fonts.fixed.style_hint, 7);
    assert_eq!(fonts.menu.style_name.as_deref(), Some("Regular"));
}

#[test]
fn colour_groups_in_kdeglobals_are_the_scheme() {
    let d = desktop(
        "[Colors:View]\nBackgroundNormal=1,2,3\n[General]\nAccentColor=233,61,61\n[KDE]\ncontrast=2\n",
        "",
    );
    assert_eq!(d.scheme_origin, SchemeOrigin::Kdeglobals);
    assert_eq!(
        d.scheme.set(ColorSet::View).background_normal,
        KdeColor::rgb8(1, 2, 3)
    );
    assert_eq!(d.scheme.contrast, 2);
    assert_eq!(d.user_accent, Some(KdeColor::rgb8(233, 61, 61)));
    assert_eq!(d.scheme.accent, Some(KdeColor::rgb8(233, 61, 61)));
}

#[test]
fn a_transparent_accent_is_no_accent() {
    assert_eq!(
        desktop("[General]\nAccentColor=0,0,0,0\n", "").user_accent,
        None
    );
    assert_eq!(
        desktop("[General]\nAccentColor=transparent\n", "").user_accent,
        None
    );
    assert_eq!(
        desktop("[General]\nAccentColor=nonsense\n", "").user_accent,
        None
    );
}

#[test]
fn animation_duration_factor_absent() {
    let d = desktop("[KDE]\nwidgetStyle=breeze\n", "");
    assert_eq!(d.animation_duration_factor, None);
    assert_eq!(d.animation_factor(), 1.0);
    assert!(!d.prefers_reduced_motion());
}

#[test]
fn animation_duration_factor_zero_asks_for_reduced_motion() {
    let d = desktop("[KDE]\nAnimationDurationFactor=0\n", "");
    assert_eq!(d.animation_duration_factor, Some(0.0));
    assert_eq!(d.animation_factor(), 0.0);
    assert!(d.prefers_reduced_motion());
}

#[test]
fn animation_duration_factor_two_doubles_the_durations() {
    let d = desktop("[KDE]\nAnimationDurationFactor=2\n", "");
    assert_eq!(d.animation_duration_factor, Some(2.0));
    assert_eq!(d.animation_factor(), 2.0);
    assert!(!d.prefers_reduced_motion());
}

#[test]
fn an_unparseable_animation_duration_factor_is_present_and_reads_as_one() {
    let d = desktop("[KDE]\nAnimationDurationFactor=fast\n", "");
    assert_eq!(d.animation_duration_factor, Some(1.0));
}

#[test]
fn the_widget_style_is_breeze_only_when_absent_empty_or_named_breeze() {
    assert!(desktop("[KDE]\nwidgetStyle=\n", "").uses_breeze_style());
    assert!(desktop("[KDE]\nwidgetStyle=BREEZE\n", "").uses_breeze_style());
    let fusion = desktop("[KDE]\nwidgetStyle=Fusion\n", "");
    assert_eq!(fusion.widget_style.as_deref(), Some("Fusion"));
    assert!(!fusion.uses_breeze_style());
}

#[test]
fn fonts_are_read_as_strings_expanded_and_unparseable_ones_keep_the_default() {
    let var = |name: &str| (name == "MONO").then(|| "Fira Mono".to_owned());
    let d = desktop_with_env(
        "[General]\nfont=Inter,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\nfixed[$e]=$MONO,9,-1,7,50,0,0,0,1,0\n\
         menuFont=garbage\ntoolBarFont=\nsmallestReadableFont=Noto Sans,7\n",
        "",
        &var,
    );
    assert_eq!(d.fonts.general.family, "Inter");
    assert_eq!(d.fonts.general.point_size, Some(11.0));
    assert_eq!(d.fonts.fixed.family, "Fira Mono");
    assert_eq!(d.fonts.fixed.weight, 400);
    assert!(d.fonts.fixed.fixed_pitch);
    assert_eq!(d.fonts.menu, KdeFonts::default().menu);
    assert_eq!(d.fonts.toolbar, KdeFonts::default().toolbar);
    assert_eq!(d.fonts.small, KdeFont::parse_qfont("Noto Sans,7").unwrap());
}

#[test]
fn breezerc_settings_are_read_with_their_ranges_and_enumerations() {
    let d = desktop(
        "",
        "[Common]\nShadowStrength=10\nShadowSize=ShadowSmall\nShadowColor=10,0,0\n\
         [Style]\nAnimationsEnabled=false\nAnimationsDuration=250\nScrollBarAddLineButtons=1\n\
         ScrollBarSubLineButtons=5\nMnemonicsMode=MN_ALWAYS\nMenuOpacity=140\nMenuItemDrawStrongFocus=false\n\
         ViewDrawFocusIndicator=0\n",
    );
    let b = d.breeze;
    assert_eq!(b.shadow_strength, 25);
    assert_eq!(b.shadow_size, ShadowSize::Small);
    assert_eq!(b.shadow_color, KdeColor::rgb8(10, 0, 0));
    assert!(!b.animations_enabled);
    assert_eq!(b.animations_duration_ms, 250);
    assert_eq!(b.scroll_bar_add_line_buttons, ScrollBarButtons::Single);
    assert_eq!(b.scroll_bar_sub_line_buttons, ScrollBarButtons::Double);
    assert_eq!(b.mnemonics_mode, MnemonicsMode::Always);
    assert_eq!(b.menu_opacity, 100);
    assert!(!b.menu_item_draw_strong_focus);
    assert!(!b.view_draw_focus_indicator);
    assert_eq!(b.progress_bar_busy_step_duration_ms, 800);
}

#[test]
fn breezerc_enumerations_accept_indices_and_fall_back_on_unknown_names() {
    let d = desktop(
        "",
        "[Common]\nShadowSize=4\n[Style]\nMnemonicsMode=sometimes\n",
    );
    assert_eq!(d.breeze.shadow_size, ShadowSize::VeryLarge);
    assert_eq!(d.breeze.mnemonics_mode, MnemonicsMode::Auto);
    let unknown = desktop("", "[Common]\nShadowSize=ShadowHuge\n");
    assert_eq!(unknown.breeze.shadow_size, ShadowSize::Large);
}

#[test]
fn an_immutable_system_breezerc_wins_over_the_user_file() {
    let locations = KdeConfigLocations::from_env(|_: &str| None);
    let expansion = locations.expansion_env(&no_env);
    let breezerc = [
        KConfig::parse("[Style][$i]\nMenuOpacity=80\n").0,
        KConfig::parse("[Style]\nMenuOpacity=50\n")
            .0
            .with_kind(LayerKind::User),
    ];
    let d = KdeDesktop::from_layers(
        &KdeDesktopLayers {
            kdeglobals: &[],
            breezerc: &breezerc,
            scheme_file: None,
        },
        &expansion,
    );
    assert_eq!(d.breeze.menu_opacity, 80);
}
