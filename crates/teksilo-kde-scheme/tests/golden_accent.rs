// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `KdeColorScheme::with_accent` against the golden vectors
//! `tools/kde_golden/vectors/accent.json`: the `kdeglobals` Plasma 6.8
//! wrote for four schemes and the ten swatches, read back key by key.
//!
//! It also checks that accenting a built-in Breeze constant gives exactly
//! what accenting the vendored stock file gives. The vectors and the
//! fixtures live outside the published crate, so this file is left out of
//! the package (the `exclude` list in `Cargo.toml`).

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use teksilo_kde_scheme::{ACCENT_SWATCHES, KConfig, KdeColor, KdeColorScheme};

const ACCENT_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/kde_golden/vectors/accent.json"
));
const BREEZE_LIGHT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/breeze-v6.8.0/BreezeLight.colors"
));
const BREEZE_DARK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/breeze-v6.8.0/BreezeDark.colors"
));

fn vectors() -> Value {
    serde_json::from_str(ACCENT_JSON).expect("accent.json parses")
}

fn cases(all: &Value) -> &[Value] {
    let list = all["cases"]
        .as_array()
        .expect("accent.json has a case list");
    assert!(!list.is_empty(), "accent.json has no case");
    list
}

/// A group name as the vectors write it (`Colors:Header][Inactive`) as a
/// group path.
fn path(name: &str) -> Vec<&str> {
    name.split("][").collect()
}

fn object<'a>(value: &'a Value, what: &str) -> &'a Map<String, Value> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("{what} is an object"))
}

fn text(value: &Value) -> &str {
    value.as_str().expect("a configuration value is a string")
}

/// The scheme file the vectors record as the input of a scheme, as
/// configuration text.
fn scheme_text(input: &Value) -> String {
    let mut out = String::new();
    for (group, keys) in object(input, "a scheme input") {
        out.push('[');
        out.push_str(group);
        out.push_str("]\n");
        for (key, value) in object(keys, "a group") {
            out.push_str(key);
            out.push('=');
            out.push_str(text(value));
            out.push('\n');
        }
    }
    out
}

fn parse_clean(text: &str) -> KdeColorScheme {
    let (scheme, diagnostics) = KdeColorScheme::parse(text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    scheme
}

fn accent_of(case: &Value) -> KdeColor {
    KdeColor::parse(text(&case["accent"])).expect("the accent is a colour")
}

/// The groups `[General]` aside: the accent procedure writes only a digest
/// there, which Teksilo leaves out (spec g.2), and the scheme name the
/// vectors hold comes from applying the scheme, not from the accent.
fn compared(group: &[&str]) -> bool {
    group != ["General"]
}

#[test]
fn every_accent_case_writes_exactly_what_plasma_wrote() {
    let all = vectors();
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in cases(&all) {
        let name = text(&case["scheme"]);
        let input = &all["schemes"][name]["input"];
        let label = format!("{name} {}", text(&case["accent"]));
        let accented = parse_clean(&scheme_text(input)).with_accent(accent_of(case));
        let written = accented.source().expect("an accented scheme has a source");

        let mut expected_keys = BTreeSet::new();
        for (group, keys) in object(&case["kdeglobals"], "kdeglobals") {
            let group = path(group);
            if !compared(&group) {
                continue;
            }
            for (key, value) in object(keys, "a group") {
                expected_keys.insert((group.join("]["), key.clone()));
                checked += 1;
                let got = written.get(&group, key);
                if got != Some(text(value)) {
                    failures.push(format!(
                        "{label} [{}] {key}: wrote {got:?}, Plasma wrote {value}",
                        group.join("][")
                    ));
                }
            }
        }
        for group in written.groups() {
            if !compared(&group) {
                continue;
            }
            for key in written.keys(&group) {
                if !expected_keys.contains(&(group.join("]["), key.to_owned())) {
                    failures.push(format!(
                        "{label} [{}] {key}: written, but Plasma did not write it",
                        group.join("][")
                    ));
                }
            }
        }
        // The accent option writes no AccentColor (spec g.2).
        if written.has_key(&["General"], "AccentColor") {
            failures.push(format!("{label}: AccentColor written"));
        }
    }
    assert!(checked > 0, "no key was checked");
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_cases_cover_every_swatch_on_every_scheme() {
    let all = vectors();
    let schemes = object(&all["schemes"], "schemes");
    for name in schemes.keys() {
        let accents: Vec<KdeColor> = cases(&all)
            .iter()
            .filter(|c| c["scheme"] == name.as_str())
            .map(accent_of)
            .collect();
        assert_eq!(accents, ACCENT_SWATCHES, "the swatches accented on {name}");
    }
}

#[test]
fn the_vector_inputs_of_the_stock_schemes_are_the_vendored_files() {
    let all = vectors();
    for (name, file) in [("BreezeLight", BREEZE_LIGHT), ("BreezeDark", BREEZE_DARK)] {
        let from_vectors = KConfig::parse(&scheme_text(&all["schemes"][name]["input"])).0;
        let from_file = KConfig::parse(file).0;
        assert_eq!(from_vectors, from_file, "{name}");
    }
}

#[test]
fn accenting_a_breeze_constant_is_accenting_its_stock_file() {
    for (constant, file) in [
        (KdeColorScheme::breeze_light(), BREEZE_LIGHT),
        (KdeColorScheme::breeze_dark(), BREEZE_DARK),
    ] {
        let file = parse_clean(file);
        for accent in ACCENT_SWATCHES.into_iter().chain([
            KdeColor::rgb8(0, 0, 0),
            KdeColor::rgb8(255, 255, 255),
            KdeColor::rgba8(10, 200, 90, 128),
            KdeColor::TRANSPARENT,
        ]) {
            let from_constant = constant.with_accent(accent);
            let from_file = file.with_accent(accent);
            assert_eq!(from_constant, from_file, "accent {accent}");
            assert_eq!(
                from_constant.source(),
                from_file.source(),
                "the written configuration for accent {accent}"
            );
        }
    }
}
