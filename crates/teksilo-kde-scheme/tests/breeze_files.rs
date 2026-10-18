// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The stock Breeze constants against the vendored breeze 6.8.0 `.colors`
//! files, and the source a parsed file keeps.
//!
//! The fixtures are LGPL-2.0-or-later and are left out of the published
//! crate, and so is this file (the `exclude` list in `Cargo.toml`): a test
//! target that reads them could not compile from the package. Every test
//! that needs no fixture lives in `scheme.rs`, which is published. The
//! last test checks that every test target reading the fixtures is
//! excluded and named in NOTICE section 1.5 and in the `exclude` comment.

use teksilo_kde_scheme::KdeColorScheme;

const BREEZE_LIGHT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/breeze-v6.8.0/BreezeLight.colors"
));
const BREEZE_DARK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/breeze-v6.8.0/BreezeDark.colors"
));
const BREEZE_CLASSIC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/breeze-v6.8.0/BreezeClassic.colors"
));

fn parse_clean(text: &str) -> KdeColorScheme {
    let (scheme, diagnostics) = KdeColorScheme::parse(text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    scheme
}

#[test]
fn the_breeze_light_constant_equals_the_breeze_light_file() {
    assert_eq!(parse_clean(BREEZE_LIGHT), KdeColorScheme::breeze_light());
}

#[test]
fn the_breeze_dark_constant_equals_the_breeze_dark_file() {
    assert_eq!(parse_clean(BREEZE_DARK), KdeColorScheme::breeze_dark());
}

#[test]
fn the_breeze_classic_constant_equals_the_breeze_classic_file() {
    assert_eq!(
        parse_clean(BREEZE_CLASSIC),
        KdeColorScheme::breeze_classic()
    );
}

// ---- Source ---------------------------------------------------------------

#[test]
fn a_parsed_scheme_keeps_its_source_and_equality_ignores_it() {
    let scheme = parse_clean(BREEZE_LIGHT);
    let source = scheme.source().unwrap();
    assert_eq!(
        source.get(&["Colors:View"], "BackgroundNormal"),
        Some("255,255,255")
    );
    assert!(!source.has_key(&["KDE"], "frameContrast"));
    assert!(!source.has_key(&["ColorEffects:Disabled"], "Enable"));
    assert!(KdeColorScheme::breeze_light().source().is_none());
    assert_eq!(scheme, KdeColorScheme::breeze_light());
}

// ---- Licensing record -----------------------------------------------------

/// The test targets of this crate that read the vendored fixtures: every
/// `tests/*.rs` whose text names the fixtures directory.
fn fixture_readers() -> Vec<String> {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut readers: Vec<String> = std::fs::read_dir(&tests)
        .expect("tests/ lists")
        .map(|entry| entry.expect("tests/ entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .filter(|path| {
            std::fs::read_to_string(path)
                .expect("test file reads")
                .contains("tests/fixtures/breeze-v6.8.0/")
        })
        .map(|path| {
            format!(
                "tests/{}",
                path.file_name().unwrap().to_str().expect("UTF-8 name")
            )
        })
        .collect();
    readers.sort();
    readers
}

/// `text` with every run of whitespace collapsed to one space, so a path
/// that a wrapped paragraph puts at a line start still matches.
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every test target that reads the LGPL fixtures is excluded from the
/// published crate, and is named both in the comment above that `exclude`
/// list and in NOTICE section 1.5, the licensing record of where the
/// fixtures are used.
#[test]
fn the_licensing_record_names_every_test_that_reads_the_fixtures() {
    let readers = fixture_readers();
    assert!(
        readers.contains(&"tests/breeze_files.rs".to_owned()),
        "the scan finds this file: {readers:?}"
    );

    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("Cargo.toml reads");
    let exclude_at = manifest.find("\nexclude = [").expect("an exclude list");
    let exclude_end = exclude_at + manifest[exclude_at..].find(']').expect("the list closes");
    let exclude_list = &manifest[exclude_at..exclude_end];
    let comment: String = manifest[..exclude_at]
        .lines()
        .rev()
        .take_while(|line| line.starts_with('#'))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| line.trim_start_matches('#'))
        .collect::<Vec<_>>()
        .join(" ");
    let comment = collapse_whitespace(&comment);

    let notice = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../NOTICE"))
        .expect("NOTICE reads");
    let section_at = notice.find("\n1.5  KDE").expect("NOTICE section 1.5");
    let section_end = section_at
        + notice[section_at..]
            .find("\n2. ")
            .expect("NOTICE section 2 follows");
    let section = collapse_whitespace(&notice[section_at..section_end]);

    for reader in &readers {
        assert!(
            exclude_list.contains(&format!("\"{reader}\"")),
            "{reader} reads the LGPL fixtures but is not in the exclude list"
        );
        assert!(
            comment.contains(reader.as_str()),
            "{reader} reads the LGPL fixtures but the comment above the exclude \
             list does not name it: {comment}"
        );
        assert!(
            section.contains(reader.as_str()),
            "{reader} reads the LGPL fixtures but NOTICE section 1.5 does not name it"
        );
    }
}
