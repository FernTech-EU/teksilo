// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Breeze typography: Plasma's default fonts mapped onto Teksilo's six
//! [`TextStyle`] slots.
//!
//! | Slot | Plasma font | Glyph size | Line box | Weight |
//! | --- | --- | --- | --- | --- |
//! | `body` | general font, Noto Sans 10 pt | 13 px | 18 px | 400 |
//! | `body_bold` | the same, Kirigami's heading weight | 13 px | 18 px | 600 |
//! | `small` | smallest readable font, Noto Sans 8 pt | 11 px | 15 px | 400 |
//! | `small_bold` | the same, bold | 11 px | 15 px | 600 |
//! | `tiny` | the smallest readable font again: KDE has no smaller role | 11 px | 15 px | 400 |
//! | `mono` | fixed font, Hack 10 pt | 13 px | 15 px | 400 |
//!
//! Nothing is tracked: letter spacing is 0 everywhere.
//!
//! ## Sizes are Qt's, not the point size's
//!
//! A 10 pt font at 96 dpi is 13⅓ px, but Qt resolves a font at a whole
//! pixel size and draws that face: 13 px. Teksilo uses the same 13 px
//! ([`face_px`]), so a label is the size it is in a Qt application next to
//! it rather than a third of a pixel larger.
//!
//! The line box follows Qt's measurement too ([`qt_line_height`]): the
//! face's ascent and descent at that pixel size, each truncated to 1/64 px
//! and rounded to a whole pixel separately, then added. For Noto Sans that
//! gives 15, 17, 18, 20, 22 and 26 px at 8, 9, 10, 11, 12 and 14 pt, the
//! heights Qt reports; a rule such as "1.35 times the size" is a pixel short
//! at several of them. The line-height ratio each style carries is that
//! whole line box over the glyph size, so the box Teksilo lays out is the
//! integer one. The control heights in [`crate::geometry`] are all derived
//! from the body line box.
//!
//! ## Families
//!
//! Plasma's faces are Noto Sans and Hack. The stock themes do not ship or
//! name them: they keep Teksilo's bundled Inter and JetBrains Mono at the
//! sizes and line boxes above, which are computed from the Noto Sans and
//! Hack metrics whichever family draws them.
//!
//! ## The desktop's fonts
//!
//! [`desktop_typography`] maps a desktop's own fonts onto the six slots:
//! the general font onto `body` and `body_bold`, the smallest readable
//! font onto `small`, `small_bold` and `tiny`, the fixed font onto `mono`.
//! Each takes the font's family as Plasma names it and its size by Qt's
//! rule above (a font stored in pixels keeps its pixel size); the weights
//! stay the slots' own, 400 and 600. The line box is Qt's for Noto Sans and
//! Hack, whose metrics are known, and `round(P x 1.362)` for any other
//! family ([`desktop_line_height`]), Noto Sans's natural line height, which
//! can be a pixel off for a face with other metrics: the preset measures no
//! font. Teksilo has no slot for the menu and toolbar fonts, so
//! [`KdeFontRoles`] carries them on the theme; the menu rows are sized for
//! the menu font ([`crate::styles::menu_item`]).

use teksilo_kde_scheme::{KdeFont, KdeFonts};
use teksilo_tokens::{FontWeight, TextStyle, TypographyTokens};

/// The vertical metrics of a font face that its line height is computed
/// from: the `hhea` ascender and the absolute descender, in font units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceMetrics {
    /// Units per em.
    pub units_per_em: u32,
    /// The `hhea` ascender.
    pub ascender: u32,
    /// The `hhea` descender, as a positive number.
    pub descender: u32,
}

/// `[measured-qt]` Noto Sans v2.015: 1000 units per em, ascender 1069,
/// descender 293, no line gap. The values Qt's line heights reproduce.
pub const NOTO_SANS_METRICS: FaceMetrics = FaceMetrics {
    units_per_em: 1000,
    ascender: 1069,
    descender: 293,
};

/// `[measured-qt]` Hack v3.003: 2048 units per em, ascender 1901, descender
/// 483, no line gap.
pub const HACK_METRICS: FaceMetrics = FaceMetrics {
    units_per_em: 2048,
    ascender: 1901,
    descender: 483,
};

/// `[measured-qt]` Plasma's general font size, in points.
pub const BREEZE_BODY_PT: f32 = 10.0;
/// `[measured-qt]` Plasma's smallest readable font size, in points.
pub const BREEZE_SMALL_PT: f32 = 8.0;
/// `[measured-qt]` Plasma's fixed font size, in points.
pub const BREEZE_MONO_PT: f32 = 10.0;

/// `[K]` The weight Kirigami gives a heading, used for the bold styles.
pub const BREEZE_BOLD_WEIGHT: FontWeight = FontWeight::SEMI_BOLD;

/// `[measured-qt]` The whole pixel size Qt resolves a font of `pt` points
/// to at 96 dpi: `pt x 96 / 72`, rounded half up. 13 for 10 pt, 11 for
/// 8 pt.
pub fn face_px(pt: f32) -> u32 {
    let px = f64::from(pt) * 96.0 / 72.0;
    (px + 0.5).floor().max(0.0) as u32
}

/// `[measured-qt]` The line height Qt reports for a face of `face_px`
/// pixels with `metrics`: the ascent `floor(64 P asc / upm) / 64` and the
/// descent likewise, each rounded half up to a whole pixel, then added.
pub fn qt_line_height(face_px: u32, metrics: FaceMetrics) -> u32 {
    let sixty_fourths = |units: u32| -> u64 {
        64 * u64::from(face_px) * u64::from(units) / u64::from(metrics.units_per_em.max(1))
    };
    let round = |v: u64| -> u64 { (v + 32) / 64 };
    (round(sixty_fourths(metrics.ascender)) + round(sixty_fourths(metrics.descender))) as u32
}

/// The body line height `H` of Plasma's default general font, 18 px: what
/// every text-driven control height in [`crate::geometry`] starts from.
pub fn breeze_body_line_height() -> f32 {
    qt_line_height(face_px(BREEZE_BODY_PT), NOTO_SANS_METRICS) as f32
}

/// Breeze typography tokens at Plasma's default fonts.
pub fn breeze_typography() -> TypographyTokens {
    // `[unpublished]` Without bundled Noto Sans and Hack, the faces are
    // Teksilo's bundled defaults, at the sizes Plasma uses.
    let mut t = TypographyTokens::default();

    let body_px = face_px(BREEZE_BODY_PT);
    let body_line = qt_line_height(body_px, NOTO_SANS_METRICS);
    let small_px = face_px(BREEZE_SMALL_PT);
    let small_line = qt_line_height(small_px, NOTO_SANS_METRICS);
    let mono_px = face_px(BREEZE_MONO_PT);
    let mono_line = qt_line_height(mono_px, HACK_METRICS);

    let set = |style: &mut teksilo_tokens::TextStyle, px: u32, line: u32, weight: FontWeight| {
        style.size = px as f32;
        style.line_height = line as f32 / px as f32;
        style.weight = weight;
        style.letter_spacing = 0.0;
    };
    set(&mut t.body, body_px, body_line, FontWeight::REGULAR);
    set(&mut t.body_bold, body_px, body_line, BREEZE_BOLD_WEIGHT);
    set(&mut t.small, small_px, small_line, FontWeight::REGULAR);
    set(&mut t.small_bold, small_px, small_line, BREEZE_BOLD_WEIGHT);
    // `[derived]` KDE has no role below the smallest readable font.
    set(&mut t.tiny, small_px, small_line, FontWeight::REGULAR);
    set(&mut t.mono, mono_px, mono_line, FontWeight::REGULAR);
    t
}

/// `[derived]` The line height of a family whose metrics the preset does
/// not know, in em: Noto Sans's natural line height, its ascender and
/// descender over its units per em, rounded to three places.
pub const NATURAL_LINE_HEIGHT_EM: f64 = 1.362;

/// The metrics of `family` when the preset knows them: Noto Sans and Hack,
/// named in any ASCII case.
pub fn known_metrics(family: &str) -> Option<FaceMetrics> {
    if family.eq_ignore_ascii_case("Noto Sans") {
        Some(NOTO_SANS_METRICS)
    } else if family.eq_ignore_ascii_case("Hack") {
        Some(HACK_METRICS)
    } else {
        None
    }
}

/// `[measured-qt]` The whole pixel size Qt draws `font` at: its pixel size
/// when it is stored in pixels, otherwise its point size by [`face_px`],
/// otherwise `default`'s. At least 1.
pub fn desktop_face_px(font: &KdeFont, default: &KdeFont) -> u32 {
    let points = |f: &KdeFont| f.point_size.map(|pt| face_px(pt as f32));
    let pixels = |f: &KdeFont| f.pixel_size.map(|px| px.max(0) as u32);
    pixels(font)
        .or_else(|| points(font))
        .or_else(|| pixels(default))
        .or_else(|| points(default))
        .unwrap_or_else(|| face_px(BREEZE_BODY_PT))
        .max(1)
}

/// The line height of a `face_px` face of `family`: [`qt_line_height`]
/// with its metrics when the preset knows them ([`known_metrics`]), and
/// `[derived]` `round(face_px x 1.362)`, rounded half up, otherwise.
pub fn desktop_line_height(face_px: u32, family: &str) -> u32 {
    match known_metrics(family) {
        Some(metrics) => qt_line_height(face_px, metrics),
        None => (f64::from(face_px) * NATURAL_LINE_HEIGHT_EM + 0.5).floor() as u32,
    }
}

/// The text style of a desktop `font` at `weight`: its family (`default`'s
/// when it names none, `[unpublished]`), the whole pixel size Qt draws it
/// at ([`desktop_face_px`]) and the line box of [`desktop_line_height`],
/// with no tracking.
pub fn desktop_text_style(font: &KdeFont, default: &KdeFont, weight: FontWeight) -> TextStyle {
    let family = if font.family.trim().is_empty() {
        default.family.clone()
    } else {
        font.family.clone()
    };
    let px = desktop_face_px(font, default);
    let line = desktop_line_height(px, &family);
    TextStyle {
        family,
        size: px as f32,
        weight,
        line_height: line as f32 / px as f32,
        letter_spacing: 0.0,
    }
}

/// Breeze typography tokens for a desktop's `fonts`. See [the module
/// documentation](self#the-desktops-fonts).
pub fn desktop_typography(fonts: &KdeFonts) -> TypographyTokens {
    let defaults = KdeFonts::default();
    let general = |weight| desktop_text_style(&fonts.general, &defaults.general, weight);
    let small = |weight| desktop_text_style(&fonts.small, &defaults.small, weight);
    TypographyTokens {
        body: general(FontWeight::REGULAR),
        body_bold: general(BREEZE_BOLD_WEIGHT),
        small: small(FontWeight::REGULAR),
        small_bold: small(BREEZE_BOLD_WEIGHT),
        // `[derived]` KDE has no role below the smallest readable font.
        tiny: small(FontWeight::REGULAR),
        mono: desktop_text_style(&fonts.fixed, &defaults.fixed, FontWeight::REGULAR),
    }
}

/// The two KDE font roles Teksilo's six text styles have no slot for, as
/// text styles: the menu font and the toolbar font.
///
/// A [`teksilo_core::styles::Theme`] extension. The menu rows the preset
/// installs are sized for [`menu`](Self::menu); `MenuItem` still writes its
/// label in the body style unless an app hands it this one, and a toolbar's
/// buttons write in the body style, so an app that wants the desktop's
/// toolbar font passes [`toolbar`](Self::toolbar) to the labels it builds.
#[derive(Debug, Clone, PartialEq)]
pub struct KdeFontRoles {
    /// `[General] menuFont`: Plasma's default, Noto Sans 10 pt.
    pub menu: TextStyle,
    /// `[General] toolBarFont`: Plasma's default, Noto Sans 10 pt.
    pub toolbar: TextStyle,
}

impl KdeFontRoles {
    /// The stock themes' roles: both are the body style of
    /// [`breeze_typography`], as Plasma's defaults are the general font.
    pub fn breeze_default() -> KdeFontRoles {
        let body = breeze_typography().body;
        KdeFontRoles {
            menu: body.clone(),
            toolbar: body,
        }
    }

    /// The roles of a desktop's `fonts`, regular weight, as
    /// [`desktop_text_style`] maps them.
    pub fn from_fonts(fonts: &KdeFonts) -> KdeFontRoles {
        let defaults = KdeFonts::default();
        KdeFontRoles {
            menu: desktop_text_style(&fonts.menu, &defaults.menu, FontWeight::REGULAR),
            toolbar: desktop_text_style(&fonts.toolbar, &defaults.toolbar, FontWeight::REGULAR),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_box(style: &teksilo_tokens::TextStyle) -> f32 {
        style.size * style.line_height
    }

    #[test]
    fn the_face_size_is_the_whole_pixel_size_qt_resolves() {
        assert_eq!(face_px(10.0), 13);
        assert_eq!(face_px(8.0), 11);
        assert_eq!(face_px(9.0), 12);
        assert_eq!(face_px(11.0), 15);
        assert_eq!(face_px(12.0), 16);
        assert_eq!(face_px(14.0), 19);
    }

    #[test]
    fn the_line_height_reproduces_every_size_qt_reported() {
        for (pt, expected) in [
            (8.0, 15),
            (9.0, 17),
            (10.0, 18),
            (11.0, 20),
            (12.0, 22),
            (14.0, 26),
        ] {
            assert_eq!(
                qt_line_height(face_px(pt), NOTO_SANS_METRICS),
                expected,
                "Noto Sans {pt} pt"
            );
        }
        for (pt, expected) in [(9.0, 14), (10.0, 15), (11.0, 18)] {
            assert_eq!(
                qt_line_height(face_px(pt), HACK_METRICS),
                expected,
                "Hack {pt} pt"
            );
        }
    }

    #[test]
    fn rounding_ascent_and_descent_together_would_be_a_pixel_short() {
        // The reason the two are rounded separately: at 9 pt the ascent
        // 12.81 and the descent 3.52 round to 13 and 4, 17 in all, while
        // their sum 16.33 rounds to 16. Qt reports 17.
        let together = |pt: f32| {
            let p = f64::from(face_px(pt));
            let m = NOTO_SANS_METRICS;
            let a = (64.0 * p * f64::from(m.ascender) / f64::from(m.units_per_em)).floor() / 64.0;
            let d = (64.0 * p * f64::from(m.descender) / f64::from(m.units_per_em)).floor() / 64.0;
            (a + d).round() as u32
        };
        assert_eq!(together(9.0), 16);
        assert_eq!(qt_line_height(face_px(9.0), NOTO_SANS_METRICS), 17);
    }

    #[test]
    fn the_styles_carry_the_whole_line_boxes() {
        let t = breeze_typography();
        assert_eq!(t.body.size, 13.0);
        assert!((line_box(&t.body) - 18.0).abs() < 1e-4);
        assert_eq!(t.body_bold.size, 13.0);
        assert!((line_box(&t.body_bold) - 18.0).abs() < 1e-4);
        for small in [&t.small, &t.small_bold, &t.tiny] {
            assert_eq!(small.size, 11.0);
            assert!((line_box(small) - 15.0).abs() < 1e-4);
        }
        assert_eq!(t.mono.size, 13.0);
        assert!((line_box(&t.mono) - 15.0).abs() < 1e-4);
        assert_eq!(breeze_body_line_height(), 18.0);
    }

    #[test]
    fn weights_and_tracking() {
        let t = breeze_typography();
        assert_eq!(t.body.weight, FontWeight::REGULAR);
        assert_eq!(t.body_bold.weight, FontWeight::SEMI_BOLD);
        assert_eq!(t.small_bold.weight, FontWeight::SEMI_BOLD);
        assert_eq!(t.tiny.weight, FontWeight::REGULAR);
        for s in [
            &t.body,
            &t.body_bold,
            &t.small,
            &t.small_bold,
            &t.tiny,
            &t.mono,
        ] {
            assert_eq!(s.letter_spacing, 0.0);
        }
    }

    #[test]
    fn plasmas_default_fonts_give_the_breeze_sizes_and_line_boxes() {
        let t = desktop_typography(&KdeFonts::default());
        let b = breeze_typography();
        for (desktop, stock) in [
            (&t.body, &b.body),
            (&t.body_bold, &b.body_bold),
            (&t.small, &b.small),
            (&t.small_bold, &b.small_bold),
            (&t.tiny, &b.tiny),
            (&t.mono, &b.mono),
        ] {
            assert_eq!(desktop.size, stock.size);
            assert_eq!(desktop.line_height, stock.line_height);
            assert_eq!(desktop.weight, stock.weight);
            assert_eq!(desktop.letter_spacing, 0.0);
        }
        assert_eq!(t.body.family, "Noto Sans");
        assert_eq!(t.small.family, "Noto Sans");
        assert_eq!(t.mono.family, "Hack");
        let roles = KdeFontRoles::from_fonts(&KdeFonts::default());
        assert_eq!(roles.menu, t.body);
        assert_eq!(roles.toolbar, t.body);
    }

    #[test]
    fn a_family_without_known_metrics_takes_noto_sans_natural_line_height() {
        // 11 pt is a 15 px face: 15 x 1.362 is 20.43.
        assert_eq!(desktop_line_height(15, "Cantarell"), 20);
        // 16 x 1.362 is 21.79, and 22 for Noto Sans by Qt's rule as well.
        assert_eq!(desktop_line_height(16, "Inter"), 22);
        assert_eq!(desktop_line_height(16, "noto sans"), 22);
        // Hack's own metrics: 15 at 13 px, where the rule would say 18.
        assert_eq!(desktop_line_height(13, "HACK"), 15);
        assert_eq!(desktop_line_height(13, "Fira Code"), 18);
    }

    #[test]
    fn a_font_stored_in_pixels_keeps_its_pixel_size() {
        let defaults = KdeFonts::default();
        let mut font = KdeFont::new("Noto Sans", 0.0);
        font.point_size = None;
        font.pixel_size = Some(17);
        assert_eq!(desktop_face_px(&font, &defaults.general), 17);
        // Neither stored: the default's 10 pt.
        font.pixel_size = None;
        assert_eq!(desktop_face_px(&font, &defaults.general), 13);
        // A tiny point size still draws a pixel.
        font.point_size = Some(0.1);
        assert_eq!(desktop_face_px(&font, &defaults.general), 1);
    }

    #[test]
    fn a_font_without_a_family_takes_the_roles_default() {
        let defaults = KdeFonts::default();
        let style = desktop_text_style(
            &KdeFont::new("", 12.0),
            &defaults.fixed,
            FontWeight::REGULAR,
        );
        assert_eq!(style.family, "Hack");
        assert_eq!(style.size, 16.0);
    }

    #[test]
    fn the_families_are_the_bundled_defaults() {
        let t = breeze_typography();
        let base = TypographyTokens::default();
        assert_eq!(t.body.family, base.body.family);
        assert_eq!(t.small.family, base.small.family);
        assert_eq!(t.mono.family, base.mono.family);
    }
}
