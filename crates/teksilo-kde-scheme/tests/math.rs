// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The worked examples of section d of Teksilo's KDE specification, the
//! 8-bit blends the golden vectors reach only through the accent oracle,
//! and Teksilo's own WCAG lift, [`lift_luma_until`].
//!
//! `tests/golden_math.rs` checks the same functions against every measured
//! record; this file needs no vector and is published with the crate.

use teksilo_kde_scheme::KdeColor;
use teksilo_kde_scheme::math::{
    self, Hcy, ShadeRole, accent_over, black_or_white_on, blend8, darken, lift_luma_until, luma,
    shade_role, tint,
};
use teksilo_tokens::contrast::composite_contrast;

fn rgb(r: u8, g: u8, b: u8) -> KdeColor {
    KdeColor::rgb8(r, g, b)
}

fn rgb8(c: KdeColor) -> [u8; 3] {
    let [r, g, b, _] = c.to_rgba8();
    [r, g, b]
}

fn wcag(fg: KdeColor, bg: KdeColor) -> f32 {
    composite_contrast(fg.to_color(), bg.to_color())
}

#[test]
fn the_breeze_accent_has_the_specified_hcy_coordinates_and_returns() {
    let accent = rgb(61, 174, 233);
    let hcy = Hcy::from_color(accent);
    assert!((hcy.h - 0.583_362_32).abs() < 1e-8, "{hcy:?}");
    assert!((hcy.c - 0.885_926_16).abs() < 1e-8, "{hcy:?}");
    assert!((hcy.y - 0.376_834_96).abs() < 1e-8, "{hcy:?}");
    assert_eq!(hcy.to_color(), accent);
}

#[test]
fn a_red_dominant_colour_with_more_blue_than_green_has_a_negative_raw_hue() {
    let hcy = Hcy::from_color(rgb(233, 58, 154));
    assert!(hcy.h < 0.0 && hcy.h > -1.0 / 6.0, "{hcy:?}");
    assert!((hcy.wrapped_hue() - (hcy.h + 1.0)).abs() < 1e-15);
    assert_eq!(hcy.to_color(), rgb(233, 58, 154));
}

#[test]
fn breeze_light_window_shades_match_the_worked_example() {
    let window = rgb(239, 240, 241);
    let expected = [
        (ShadeRole::Light, [255, 255, 255]),
        (ShadeRole::Midlight, [246, 247, 247]),
        (ShadeRole::Mid, [196, 200, 204]),
        (ShadeRole::Dark, [136, 142, 147]),
        (ShadeRole::Shadow, [71, 74, 76]),
    ];
    for (role, want) in expected {
        assert_eq!(rgb8(shade_role(window, role, 0.4, 0.0)), want, "{role:?}");
    }
}

#[test]
fn the_selection_tints_of_breeze_light_match_the_worked_examples() {
    let accent = rgb(61, 174, 233);
    assert_eq!(rgb8(tint(rgb(239, 240, 241), accent, 0.4)), [194, 224, 245]);
    assert_eq!(rgb8(tint(rgb(227, 229, 231), accent, 0.4)), [176, 216, 243]);
}

#[test]
fn the_link_darkening_matches_the_worked_example() {
    let accent = rgb(61, 174, 233);
    assert!(math::color_luma_ratio(accent, KdeColor::WHITE) < 5.0);
    assert_eq!(rgb8(darken(accent, 0.5, 1.0)), [45, 127, 170]);
}

#[test]
fn blend8_truncates_rather_than_rounds() {
    // Section d.12: white at 0.6 over white paints 254, not 255.
    let white_at_06 = KdeColor {
        a: math::store_channel(0.6),
        ..KdeColor::WHITE
    };
    assert_eq!(white_at_06.a, 39321);
    assert_eq!(
        blend8(white_at_06, KdeColor::WHITE).to_rgba8(),
        [254, 254, 254, 255]
    );
}

#[test]
fn blend8_returns_the_background_for_a_transparent_foreground() {
    let background = KdeColor::rgba16(1000, 2000, 3000, 40000);
    assert_eq!(blend8(KdeColor::TRANSPARENT, background), background);
}

#[test]
fn blend8_over_a_translucent_background_keeps_kde_s_nearly_transparent_alpha() {
    let fg = KdeColor::rgba8(200, 100, 50, 128);
    let bg = KdeColor::rgba8(10, 20, 30, 128);
    let out = blend8(fg, bg);
    let [_, _, _, a] = out.to_rgba8();
    assert!(a <= 1, "alpha {a} of {out:?}");
}

#[test]
fn the_accent_over_a_background_matches_the_worked_examples() {
    let accent = rgb(61, 174, 233);
    assert_eq!(rgb8(accent_over(accent, KdeColor::WHITE)), [119, 198, 239]);
    assert_eq!(rgb8(accent_over(accent, rgb(20, 22, 24))), [48, 128, 170]);
    assert_eq!(
        rgb8(accent_over(accent, rgb(252, 252, 252))),
        [118, 197, 238]
    );
    // Only the accent's 8-bit channels count: its own alpha is replaced.
    let translucent = KdeColor::rgba8(61, 174, 233, 10);
    assert_eq!(
        accent_over(translucent, KdeColor::WHITE),
        accent_over(accent, KdeColor::WHITE)
    );
}

#[test]
fn the_ten_swatches_give_the_specified_selection_and_text() {
    let light = KdeColor::WHITE;
    let dark = rgb(20, 22, 24);
    // (swatch, light selection, black text on it, dark selection, black text on it)
    let table = [
        ([233, 58, 154], [239, 117, 184], true, [169, 47, 115], false),
        ([233, 61, 88], [239, 119, 138], true, [169, 49, 68], false),
        ([233, 100, 58], [239, 146, 117], true, [169, 76, 47], false),
        ([232, 203, 45], [238, 218, 107], true, [168, 148, 38], true),
        ([61, 212, 37], [119, 224, 102], true, [48, 155, 33], true),
        ([0, 211, 184], [76, 224, 205], true, [5, 154, 136], true),
        ([61, 174, 233], [119, 198, 239], true, [48, 128, 170], true),
        (
            [184, 117, 220],
            [205, 158, 230],
            true,
            [134, 88, 161],
            false,
        ),
        (
            [146, 110, 228],
            [178, 153, 236],
            true,
            [108, 83, 166],
            false,
        ),
        ([104, 107, 111], [149, 151, 154], true, [78, 81, 84], false),
    ];
    for ([r, g, b], on_light, light_black, on_dark, dark_black) in table {
        let swatch = rgb(r, g, b);
        for (backdrop, selection, black) in
            [(light, on_light, light_black), (dark, on_dark, dark_black)]
        {
            let fill = accent_over(swatch, backdrop);
            assert_eq!(rgb8(fill), selection, "swatch {swatch}");
            let text = black_or_white_on(fill, true);
            let want = if black {
                KdeColor::BLACK
            } else {
                KdeColor::WHITE
            };
            assert_eq!(text, want, "text on {fill} for swatch {swatch}");
        }
    }
}

#[test]
fn inactive_text_is_the_chosen_colour_at_60_percent_over_the_background() {
    let fill = rgb(119, 198, 239);
    let text = black_or_white_on(fill, false);
    // Black at 0.6 over the fill: each channel truncated to 40 % of it.
    assert_eq!(rgb8(text), [47, 79, 95]);
    assert_eq!(text.to_rgba8()[3], 255);
}

#[test]
fn black_text_starts_just_above_the_luma_threshold() {
    let threshold = 1.05 / 4.5 - 0.05;
    for v in 0..=255_u8 {
        let grey = rgb(v, v, v);
        let black = black_or_white_on(grey, true) == KdeColor::BLACK;
        assert_eq!(black, luma(grey) > threshold, "grey {v}");
    }
}

#[test]
fn an_identical_tint_target_returns_the_base_colour_s_luma() {
    let base = rgb(35, 38, 41);
    assert_eq!(rgb8(math::oklab_tint(base, base, 0.25)), [35, 38, 41]);
}

// Teksilo's WCAG lift.

#[test]
fn a_colour_that_already_passes_is_unchanged() {
    let fg = rgb(35, 38, 41);
    assert_eq!(lift_luma_until(fg, &[KdeColor::WHITE], 4.5), fg);
}

#[test]
fn no_backdrop_means_no_change() {
    let fg = rgb(200, 200, 200);
    assert_eq!(lift_luma_until(fg, &[], 4.5), fg);
}

#[test]
fn a_light_accent_on_white_darkens_to_the_floor_and_keeps_its_hue() {
    let accent = rgb(61, 174, 233);
    assert!(wcag(accent, KdeColor::WHITE) < 4.5);
    let lifted = lift_luma_until(accent, &[KdeColor::WHITE], 4.5);
    let contrast = wcag(lifted, KdeColor::WHITE);
    assert!(contrast >= 4.5, "{lifted} reaches {contrast}");
    // The first passing step, so not much past the floor.
    assert!(contrast < 4.7, "{lifted} overshoots to {contrast}");
    assert!(luma(lifted) < luma(accent));
    let (before, after) = (Hcy::from_color(accent), Hcy::from_color(lifted));
    assert!(
        (before.wrapped_hue() - after.wrapped_hue()).abs() < 0.01,
        "{before:?} {after:?}"
    );
    assert!((before.c - after.c).abs() < 0.02, "{before:?} {after:?}");
}

#[test]
fn a_dark_colour_on_a_dark_backdrop_lightens() {
    let fg = rgb(41, 128, 185);
    let bg = rgb(20, 22, 24);
    let lifted = lift_luma_until(fg, &[bg], 7.0);
    assert!(wcag(lifted, bg) >= 7.0, "{lifted}");
    assert!(luma(lifted) > luma(fg));
}

#[test]
fn the_floor_holds_against_every_backdrop() {
    let fg = rgb(150, 150, 150);
    let backdrops = [KdeColor::WHITE, rgb(239, 240, 241), rgb(222, 224, 226)];
    let lifted = lift_luma_until(fg, &backdrops, 4.5);
    for bg in backdrops {
        assert!(wcag(lifted, bg) >= 4.5, "{lifted} on {bg}");
    }
}

#[test]
fn an_unreachable_floor_returns_the_best_reachable_colour() {
    let fg = rgb(61, 174, 233);
    let grey = rgb(119, 119, 119);
    let lifted = lift_luma_until(fg, &[grey], 21.0);
    let best = wcag(lifted, grey);
    assert!(best < 21.0);
    // Black is the strongest the lift can reach on this grey, and it gets
    // there: its luma end is black whatever the chroma.
    assert!(
        (best - wcag(KdeColor::BLACK, grey)).abs() < 1e-4,
        "{lifted} reaches {best}"
    );
}

#[test]
fn the_lift_keeps_alpha() {
    let fg = KdeColor::rgba8(61, 174, 233, 200);
    let lifted = lift_luma_until(fg, &[KdeColor::WHITE], 3.0);
    assert_eq!(lifted.a, fg.a);
    assert!(wcag(lifted, KdeColor::WHITE) >= 3.0, "{lifted}");
}
