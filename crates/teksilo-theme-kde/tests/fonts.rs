// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Plasma's faces: the files under `fonts/`, the families the typography
//! names with and without a font feature, and the registrar
//! `bundled-fonts` adds.
//!
//! The file tests run in every build: the files ship in the package
//! whichever features are on.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use teksilo_theme_kde as kde;
use teksilo_theme_kde::typography::{FaceMetrics, HACK_METRICS, NOTO_SANS_METRICS};

/// Every file under `fonts/` and the sha256 recorded for it in `NOTICE`,
/// sections 2.6 and 2.7. `NotoSans-Variable.ttf` and `Hack-Regular.ttf`
/// carry the hashes the specification's ledger gives (o.10); every file
/// came out of an upstream archive whose hash it gives too.
const FILES: [(&str, &str); 8] = [
    (
        "NotoSans-Variable.ttf",
        "ae80a5e79e4afbf493e9f4684ec4988e6a861b3598102ee48950b4b7a754cdff",
    ),
    (
        "NotoSans-Italic-Variable.ttf",
        "8bcb0ae904c973257ec957ca92559d425469d92e0913d489fe80fbb49c3b6ded",
    ),
    (
        "noto-sans-LICENSE.txt",
        "cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a",
    ),
    (
        "Hack-Regular.ttf",
        "15f55cc0c85a2988d2b4b3a8cdb5d77fdfbaf319e1bb5309d725db9818fb7125",
    ),
    (
        "Hack-Bold.ttf",
        "5bbf531eff7f8a0c2559c9a0656718e2828a012a9b1f60b5f54006d59a4de8d4",
    ),
    (
        "Hack-Italic.ttf",
        "096fb67a2b85f3c866e9cb3e965b27c2c10b977315f4d3d7f095674be35091c1",
    ),
    (
        "Hack-BoldItalic.ttf",
        "64f74a079700b7dfe128551a1e28875d5ba980971e55f5e0f0596e37bdc6a6bc",
    ),
    (
        "hack-LICENSE.txt",
        "1f61bb7c790c59b4b0ecdf304628b94e42ae4c8020094a8c3da73381ab212623",
    ),
];

fn fonts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts")
}

fn read(name: &str) -> Vec<u8> {
    let path = fonts_dir().join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn every_file_is_the_one_recorded() {
    for (name, expected) in FILES {
        assert_eq!(sha256_hex(&read(name)), expected, "{name}");
    }
}

#[test]
fn the_fonts_directory_holds_only_the_recorded_files() {
    let mut present: Vec<String> = std::fs::read_dir(fonts_dir())
        .expect("the fonts directory")
        .map(|e| e.expect("a directory entry").file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    present.sort();
    let mut recorded: Vec<String> = FILES.iter().map(|(n, _)| (*n).to_owned()).collect();
    recorded.sort();
    assert_eq!(present, recorded);
}

/// The table `tag` of a TrueType file: its offset in `font`.
fn table(font: &[u8], tag: &[u8; 4]) -> usize {
    let u16_at = |at: usize| u16::from_be_bytes([font[at], font[at + 1]]);
    let u32_at =
        |at: usize| u32::from_be_bytes([font[at], font[at + 1], font[at + 2], font[at + 3]]);
    let tables = usize::from(u16_at(4));
    (0..tables)
        .map(|i| 12 + 16 * i)
        .find(|&record| &font[record..record + 4] == tag)
        .map(|record| u32_at(record + 8) as usize)
        .unwrap_or_else(|| panic!("no {} table", String::from_utf8_lossy(tag)))
}

/// The metrics a line box is computed from, read from the file's `head`
/// and `hhea` tables, and the `hhea` line gap.
fn metrics_of(font: &[u8]) -> (FaceMetrics, i16) {
    let i16_at = |at: usize| i16::from_be_bytes([font[at], font[at + 1]]);
    let head = table(font, b"head");
    let hhea = table(font, b"hhea");
    let units_per_em = u16::from_be_bytes([font[head + 18], font[head + 19]]);
    let ascender = i16_at(hhea + 4);
    let descender = i16_at(hhea + 6);
    let line_gap = i16_at(hhea + 8);
    (
        FaceMetrics {
            units_per_em: u32::from(units_per_em),
            ascender: u32::try_from(ascender).expect("a positive ascender"),
            descender: u32::try_from(-i32::from(descender)).expect("a negative descender"),
        },
        line_gap,
    )
}

/// The line boxes are computed from constants; these are the files the
/// constants describe, upright and italic, regular and bold alike.
#[test]
fn the_line_boxes_are_computed_from_the_bundled_faces_metrics() {
    for name in ["NotoSans-Variable.ttf", "NotoSans-Italic-Variable.ttf"] {
        assert_eq!(metrics_of(&read(name)), (NOTO_SANS_METRICS, 0), "{name}");
    }
    for name in [
        "Hack-Regular.ttf",
        "Hack-Bold.ttf",
        "Hack-Italic.ttf",
        "Hack-BoldItalic.ttf",
    ] {
        assert_eq!(metrics_of(&read(name)), (HACK_METRICS, 0), "{name}");
    }
    // Hack's natural line height, (1901 + 483) / 2048 em, is 1.1640625:
    // 15.13 px at the 13 px face, 15 by Qt's rule, the monospace line box.
    let natural = f64::from(HACK_METRICS.ascender + HACK_METRICS.descender)
        / f64::from(HACK_METRICS.units_per_em);
    assert_eq!(natural, 1.1640625);
    let mono = kde::light().typography.mono;
    assert_eq!(mono.size, 13.0);
    assert!((mono.size * mono.line_height - 15.0).abs() < 1e-4);
}

/// The sizes and line boxes do not depend on the features: only the
/// families do.
#[test]
fn the_sizes_and_line_boxes_are_the_same_in_every_build() {
    let t = kde::light().typography;
    for (style, size, line) in [
        (&t.body, 13.0, 18.0),
        (&t.body_bold, 13.0, 18.0),
        (&t.small, 11.0, 15.0),
        (&t.small_bold, 11.0, 15.0),
        (&t.tiny, 11.0, 15.0),
        (&t.mono, 13.0, 15.0),
    ] {
        assert_eq!(style.size, size);
        assert!((style.size * style.line_height - line).abs() < 1e-4);
    }
}

#[cfg(any(feature = "bundled-fonts", feature = "system-fonts"))]
mod with_a_font_feature {
    use teksilo_kde_scheme::{KdeColorScheme, KdeDesktop};
    use teksilo_theme_kde::KdeFontRoles;
    use teksilo_theme_kde::fonts::{HACK_FAMILY, NOTO_SANS_FAMILY};
    use teksilo_tokens::TargetDensity;

    use super::kde;

    #[test]
    fn the_stock_themes_name_noto_sans_and_hack() {
        for theme in [kde::light(), kde::dark()] {
            let t = &theme.typography;
            for style in [&t.body, &t.body_bold, &t.small, &t.small_bold, &t.tiny] {
                assert_eq!(style.family, NOTO_SANS_FAMILY);
            }
            assert_eq!(t.mono.family, HACK_FAMILY);
            let roles = theme.extension::<KdeFontRoles>().expect("the font roles");
            assert_eq!(roles.menu.family, NOTO_SANS_FAMILY);
            assert_eq!(roles.toolbar.family, NOTO_SANS_FAMILY);
        }
    }

    /// With Plasma's faces named, a desktop at Plasma's default fonts has
    /// the stock theme's typography exactly, families included.
    #[test]
    fn a_desktop_at_plasmas_defaults_has_the_stock_typography() {
        let desktop = kde::from_desktop(
            &KdeDesktop::new(KdeColorScheme::breeze_light()),
            TargetDensity::Compact,
        );
        let stock = kde::light();
        assert_eq!(desktop.typography, stock.typography);
        assert_eq!(
            desktop.extension::<KdeFontRoles>(),
            stock.extension::<KdeFontRoles>()
        );
    }
}

#[cfg(not(any(feature = "bundled-fonts", feature = "system-fonts")))]
mod without_a_font_feature {
    use teksilo_tokens::TypographyTokens;

    use super::kde;

    #[test]
    fn the_stock_themes_keep_the_bundled_inter_and_jetbrains_mono() {
        let base = TypographyTokens::default();
        assert_eq!(base.body.family, "Inter");
        assert_eq!(base.mono.family, "JetBrains Mono");
        for theme in [kde::light(), kde::dark()] {
            let t = &theme.typography;
            for style in [&t.body, &t.body_bold, &t.small, &t.small_bold, &t.tiny] {
                assert_eq!(style.family, base.body.family);
            }
            assert_eq!(t.mono.family, base.mono.family);
        }
    }
}

#[cfg(feature = "bundled-fonts")]
mod bundled {
    use teksilo_text::{FontRegistrar, TextFontService};
    use teksilo_theme_kde::fonts::{HACK_FAMILY, NOTO_SANS_FAMILY};

    use super::kde;

    #[test]
    fn the_registrar_registers_both_families_and_defaults_to_noto_sans() {
        let mut service = TextFontService::new_without_system_fonts();
        let body = kde::font_registrar()
            .register_on_service(&mut service)
            .expect("the default face");
        assert_eq!(
            service.font_family_name(body).as_deref(),
            Some(NOTO_SANS_FAMILY)
        );
        let registry = service.font_registry();
        assert_eq!(registry.default_font(), Some(body));
        assert_eq!(registry.default_size_px(), 13.0);

        // Noto Sans: one variable face upright, one italic, answering every
        // weight the text styles ask for.
        for weight in [400, 600] {
            assert_eq!(
                registry.query_font(NOTO_SANS_FAMILY, weight, false),
                Some(body)
            );
        }
        let italic = registry
            .query_font(NOTO_SANS_FAMILY, 400, true)
            .expect("an italic Noto Sans");
        assert_ne!(italic, body);

        // Hack: four static faces, the bold weights going to the bold face.
        let hack = |weight, italic| {
            registry
                .query_font(HACK_FAMILY, weight, italic)
                .expect("a Hack face")
        };
        let faces = [
            hack(400, false),
            hack(700, false),
            hack(400, true),
            hack(700, true),
        ];
        for (i, a) in faces.iter().enumerate() {
            for b in &faces[i + 1..] {
                assert_ne!(a, b);
            }
            assert_ne!(*a, body);
            assert_eq!(service.font_family_name(*a).as_deref(), Some(HACK_FAMILY));
        }
        assert_eq!(hack(600, false), faces[1]);
        assert!(service.family_is_monospaced(HACK_FAMILY));
        assert!(!service.family_is_monospaced(NOTO_SANS_FAMILY));
    }
}
