// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The stock Breeze constants against the vendored breeze 6.8.0 `.colors`
//! files, and the source a parsed file keeps.
//!
//! The fixtures are LGPL-2.0-or-later and are left out of the published
//! crate, and so is this file (the `exclude` list in `Cargo.toml`): a test
//! target that reads them could not compile from the package. Every test
//! that needs no fixture lives in `scheme.rs`, which is published.

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
