// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Colour values and Qt font descriptions.

use teksilo_kde_scheme::{FontStyle, KdeColor, KdeFont};
use teksilo_tokens::Color;

// ---- Colours -------------------------------------------------------------

#[test]
fn comma_forms_give_eight_bit_channels_with_optional_alpha_signs_and_spaces() {
    assert_eq!(
        KdeColor::parse("61,174,233"),
        Some(KdeColor::rgb8(61, 174, 233))
    );
    assert_eq!(
        KdeColor::parse(" 61 , 174 ,233 "),
        Some(KdeColor::rgb8(61, 174, 233))
    );
    assert_eq!(
        KdeColor::parse("+1,-0,2,128"),
        Some(KdeColor::rgba8(1, 0, 2, 128))
    );
    assert_eq!(KdeColor::parse("1,2,3").unwrap().a, u16::MAX);
}

#[test]
fn malformed_comma_forms_are_absent() {
    for text in [
        "1,2",
        "1,2,3,4,5",
        "256,0,0",
        "-1,0,0",
        "1.5,2,3",
        "a,b,c",
        "1,,3",
        "1,2,3,",
    ] {
        assert_eq!(KdeColor::parse(text), None, "{text}");
    }
}

#[test]
fn hex_forms_follow_the_documented_widths() {
    assert_eq!(
        KdeColor::parse("#3daee9"),
        Some(KdeColor::rgb8(61, 174, 233))
    );
    assert_eq!(
        KdeColor::parse("#3DAEE9"),
        Some(KdeColor::rgb8(61, 174, 233))
    );
    assert_eq!(KdeColor::parse("#fa0"), Some(KdeColor::rgb8(255, 170, 0)));
    assert_eq!(
        KdeColor::parse("#803daee9"),
        Some(KdeColor::rgba8(61, 174, 233, 128))
    );
    // 12-bit channels: v becomes 16 v + floor(v / 256).
    assert_eq!(
        KdeColor::parse("#fff800000"),
        Some(KdeColor::rgba16(65535, 32776, 0, u16::MAX))
    );
    assert_eq!(
        KdeColor::parse("#123456789abc"),
        Some(KdeColor::rgba16(0x1234, 0x5678, 0x9abc, u16::MAX))
    );
    for text in ["#", "#12", "#12345", "#1234567", "#12345g", "# 3daee9"] {
        assert_eq!(KdeColor::parse(text), None, "{text}");
    }
}

#[test]
fn svg_names_and_transparent_are_recognised_ignoring_case() {
    assert_eq!(KdeColor::parse("red"), Some(KdeColor::rgb8(255, 0, 0)));
    assert_eq!(
        KdeColor::parse("CornflowerBlue"),
        Some(KdeColor::rgb8(100, 149, 237))
    );
    assert_eq!(KdeColor::parse("grey"), KdeColor::parse("gray"));
    assert_eq!(KdeColor::parse("Transparent"), Some(KdeColor::TRANSPARENT));
    assert!(KdeColor::TRANSPARENT.is_transparent_black());
    // CSS-only, not SVG 1.1.
    assert_eq!(KdeColor::parse("rebeccapurple"), None);
    assert_eq!(KdeColor::parse("notacolour"), None);
}

#[test]
fn empty_and_invalid_values_are_absent() {
    assert_eq!(KdeColor::parse(""), None);
    assert_eq!(KdeColor::parse("invalid"), None);
}

#[test]
fn colours_are_written_as_kde_writes_them() {
    assert_eq!(KdeColor::rgb8(61, 174, 233).to_kconfig_text(), "61,174,233");
    assert_eq!(
        KdeColor::rgba8(61, 174, 233, 178).to_string(),
        "61,174,233,178"
    );
    assert_eq!(
        KdeColor::rgba16(7067, 0, 65535, 65535).to_kconfig_text(),
        "27,0,255"
    );
}

#[test]
fn colours_cross_the_boundary_to_teksilo_colors_channel_by_channel() {
    let c = KdeColor::rgba8(255, 0, 51, 255).to_color();
    assert_eq!(c, Color::new(1.0, 0.0, 0.2, 1.0));
    let back = KdeColor::from_color(c);
    assert_eq!(back, KdeColor::rgba8(255, 0, 51, 255));
    assert_eq!(
        KdeColor::from_color(Color::new(f32::NAN, 2.0, -1.0, 0.5)).to_rgba8(),
        [0, 255, 0, 128]
    );
    let converted: Color = KdeColor::WHITE.into();
    assert_eq!(converted, Color::new(1.0, 1.0, 1.0, 1.0));
}

// ---- Fonts --------------------------------------------------------------

#[test]
fn a_plasma_six_font_string_reads_the_opentype_weight() {
    let font = KdeFont::parse_qfont("Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1").unwrap();
    assert_eq!(font.family, "Noto Sans");
    assert_eq!(font.point_size, Some(10.0));
    assert_eq!(font.pixel_size, None);
    assert_eq!(font.style_hint, 5);
    assert_eq!(font.weight, 400);
    assert_eq!(font.style, FontStyle::Normal);
    assert_eq!(font.style_strategy, 1);
    assert_eq!(font.style_name, None);
    assert_eq!(font.size_px(), Some(10.0 * 96.0 / 72.0));
}

#[test]
fn a_qt_six_string_with_a_style_name_and_spacing() {
    let font =
        KdeFont::parse_qfont("Hack,9.5,-1,7,700,1,1,1,1,0,2,1,1.5,2.5,100,1,Bold Italic").unwrap();
    assert_eq!(font.weight, 700);
    assert_eq!(font.style, FontStyle::Italic);
    assert!(font.underline && font.strike_out && font.fixed_pitch);
    assert_eq!(font.capitalization, 2);
    assert_eq!(font.letter_spacing_type, 1);
    assert_eq!(font.letter_spacing, 1.5);
    assert_eq!(font.word_spacing, 2.5);
    assert_eq!(font.stretch, 100);
    assert_eq!(font.style_name.as_deref(), Some("Bold Italic"));
}

#[test]
fn a_newer_qt_string_is_cut_to_its_first_seventeen_fields() {
    let font = KdeFont::parse_qfont(
        "Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,Regular,feature=1,axis=2",
    )
    .unwrap();
    assert_eq!(font.style_name.as_deref(), Some("Regular"));
}

#[test]
fn a_plasma_five_font_string_maps_the_legacy_weight() {
    let font = KdeFont::parse_qfont("Noto Sans,10,-1,5,50,0,0,0,0,0").unwrap();
    assert_eq!(font.weight, 400);
    let bold = KdeFont::parse_qfont("Noto Sans,10,-1,5,75,2,0,0,0,0,Bold Oblique").unwrap();
    assert_eq!(bold.weight, 700);
    assert_eq!(bold.style, FontStyle::Oblique);
    assert_eq!(bold.style_name.as_deref(), Some("Bold Oblique"));
    assert_eq!(
        KdeFont::parse_qfont("X,10,-1,5,60,0,0,0,0,0")
            .unwrap()
            .weight,
        500
    );
    assert_eq!(
        KdeFont::parse_qfont("X,10,-1,5,70,0,0,0,0,0")
            .unwrap()
            .weight,
        700
    );
}

#[test]
fn extreme_weights_in_a_font_string_are_read_without_overflow() {
    let weight = |text: &str| KdeFont::parse_qfont(text).unwrap().weight;
    assert_eq!(weight("A,10,-1,5,-2147483648,0,0,0,0,0"), 100);
    assert_eq!(weight("A,10,-1,5,2147483647,0,0,0,0,0"), 900);
    assert_eq!(weight("A,10,-1,5,-2147483648,0,0,0,0,0,0,0,0,0,0,1"), 1);
    assert_eq!(weight("A,10,-1,5,2147483647,0,0,0,0,0,0,0,0,0,0,1"), 1000);
}

#[test]
fn a_two_field_string_and_a_pixel_size() {
    let short = KdeFont::parse_qfont("Hack,11").unwrap();
    assert_eq!(short.point_size, Some(11.0));
    assert_eq!(short.weight, 400);
    let pixels = KdeFont::parse_qfont("Noto Sans,-1,14,5,400,0,0,0,0,0,0,0,0,0,0,1").unwrap();
    assert_eq!(pixels.point_size, None);
    assert_eq!(pixels.pixel_size, Some(14));
    assert_eq!(pixels.size_px(), Some(14.0));
}

#[test]
fn other_field_counts_and_unparseable_fields_are_rejected() {
    for text in [
        "",
        "Noto Sans",
        "Noto Sans,10,-1",
        "Noto Sans,10,-1,5,400,0,0,0,0,0,0,0",
        "Noto Sans,ten",
        "Noto Sans,10,-1,5,heavy,0,0,0,0,0",
    ] {
        assert_eq!(KdeFont::parse_qfont(text), None, "{text}");
    }
}
