// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property tests for the configuration parser and the readers built on it
//! (crates/teksilo-kde-scheme/src/config.rs, color.rs, fonts.rs and
//! scheme.rs).
//!
//! The parser reads files a user, a distribution or a theme package wrote,
//! so it must return rather than unwind on any input at all. `cargo-fuzz`
//! needs nightly and libfuzzer-sys, which is not assumed here; proptest
//! with 1024 cases per property gives the "never panics on weird input"
//! coverage a fuzz corpus would, plus shrinking to a minimal
//! counterexample. Manual override knob: `PROPTEST_CASES=N cargo test -p
//! teksilo-kde-scheme --test prop_config`.
//!
//! Safe run (engineering/docs/property-testing.md):
//!
//! ```text
//! cargo test -p teksilo-kde-scheme --test prop_config --no-run
//! BIN=$(cargo test -p teksilo-kde-scheme --test prop_config --no-run --message-format=json 2>/dev/null \
//!   | jq -r 'select(.executable != null) | .executable' | tail -1)
//! ( ulimit -v 6000000 -t 300; "$BIN" --test-threads=1 )
//! ```

use proptest::prelude::*;
use teksilo_kde_scheme::{KConfig, KdeColor, KdeColorScheme, KdeFont};

/// Bytes biased toward the characters the grammar gives a meaning to, so
/// most cases exercise headers, suffixes and escapes rather than plain
/// text. Cost: at most 400 bytes per case, parsed once (linear), so the
/// worst case is a few microseconds and a few kilobytes.
fn arb_config_bytes() -> impl Strategy<Value = Vec<u8>> {
    let byte = prop_oneof![
        4 => prop::sample::select(b"[]$=\\\n\r#ideftxs0123456789abcdefABCDEF,;: \t".to_vec()),
        1 => any::<u8>(),
    ];
    prop::collection::vec(byte, 0..400)
}

/// Lines assembled from grammar fragments and the keys a scheme reads, so
/// headers, nested groups, key suffixes and colour keys appear far more
/// often than random bytes would make them. Cost: at most 40 lines of up to
/// 5 fragments each, well under 4 KB, parsed, cascaded, written and parsed
/// again: a few hundred microseconds per case.
fn arb_config_text() -> impl Strategy<Value = String> {
    let fragment = prop::sample::select(vec![
        "[",
        "]",
        "[$i]",
        "[$d]",
        "[$e]",
        "[$ie]",
        "[de]",
        "[Colors:View]",
        "[Colors:Window]",
        "[Colors:Header]",
        "[Inactive]",
        "[General]",
        "[KDE]",
        "[ColorEffects:Inactive]",
        "Key",
        "BackgroundNormal",
        "BackgroundAlternate",
        "AccentColor",
        "Name",
        "contrast",
        "TitlebarIsAccentColored",
        "ColorEffect",
        "Color",
        "Enable",
        "true",
        "2",
        "=",
        "\\",
        "\\s",
        "\\x4",
        "\\xzz",
        "\\q",
        "$",
        "${HOME}",
        "$(echo)",
        "#",
        " ",
        "\r",
        "\t",
        "61,174,233",
        "#3daee9",
        "\u{FEFF}",
        "é",
    ]);
    let line = prop::collection::vec(fragment, 0..6).prop_map(|parts| parts.concat());
    prop::collection::vec(line, 0..40).prop_map(|lines| lines.join("\n"))
}

// ── 1. parsing arbitrary bytes never panics, every diagnostic is on a line
//      the input has, and the error limit is respected ──
proptest! {
    // 1024 cases: cheap per case, and a panic-freedom sweep over input a
    // user or package controls is worth the extra depth.
    #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]
    #[test]
    fn parse_never_panics_on_arbitrary_bytes(bytes in arb_config_bytes()) {
        let (_, diagnostics) = KConfig::parse_bytes(&bytes);
        let lines = bytes.split(|&b| b == b'\n').count();
        for d in &diagnostics {
            prop_assert!(d.line >= 1 && d.line <= lines, "diagnostic {d:?} outside 1..={lines} for {bytes:?}");
        }
        prop_assert!(diagnostics.len() <= 101 + lines, "{} diagnostics for {} lines", diagnostics.len(), lines);
    }
}

// ── 2. parsing, cascading and reading a scheme, fonts and colours from
//      grammar-shaped text never panics ──
proptest! {
    #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]
    #[test]
    fn every_reader_survives_grammar_shaped_text(a in arb_config_text(), b in arb_config_text()) {
        let (low, _) = KConfig::parse(&a);
        let (high, _) = KConfig::parse(&b);
        let merged = KConfig::cascade(&[low, high.with_kind(teksilo_kde_scheme::LayerKind::User)]);
        let scheme = KdeColorScheme::from_config(&merged);
        let text = scheme.to_kconfig_text();
        let (again, diagnostics) = KdeColorScheme::parse(&text);
        prop_assert!(diagnostics.is_empty(), "written text has diagnostics {diagnostics:?}:\n{text}");
        prop_assert_eq!(&again, &scheme, "round trip changed the scheme read from {:?} over {:?}", a, b);
        for line in a.lines().chain(b.lines()) {
            let _ = KdeColor::parse(line);
            let _ = KdeFont::parse_qfont(line);
        }
    }
}

/// An integer field of a font string: mostly the ends of the `i32` range
/// and their neighbours, where arithmetic on a field overflows, plus the
/// small values Qt writes and an arbitrary `i32`.
fn arb_font_int() -> impl Strategy<Value = String> {
    prop_oneof![
        2 => prop::sample::select(vec![
            i32::MIN,
            i32::MIN + 1,
            i32::MIN / 2,
            -1,
            0,
            1,
            50,
            400,
            1000,
            i32::MAX / 2,
            i32::MAX - 1,
            i32::MAX,
        ])
        .prop_map(|v| v.to_string()),
        1 => any::<i32>().prop_map(|v| v.to_string()),
        1 => prop::sample::select(vec![
            "2147483648".to_owned(),
            "-2147483649".to_owned(),
            "99999999999999999999".to_owned(),
            String::new(),
            "x".to_owned(),
        ]),
    ]
}

/// A floating-point field of a font string: the extremes of `f64`, values
/// that do not parse as finite, and ordinary sizes.
fn arb_font_float() -> impl Strategy<Value = String> {
    prop_oneof![
        2 => prop::sample::select(vec![
            "10", "9.5", "-1", "0", "1e308", "-1e308", "1e309", "inf", "NaN", "",
        ])
        .prop_map(str::to_owned),
        1 => any::<f64>().prop_map(|v| v.to_string()),
    ]
}

// ── 3. a font string of any accepted length with extreme numeric fields
//      parses without panicking, and its weight is on the OpenType scale ──
proptest! {
    // 1024 cases: a string of at most 18 short fields, parsed once, costs
    // a few microseconds; the extra depth goes into the integer extremes.
    #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]
    #[test]
    fn font_strings_with_extreme_numbers_never_panic(
        count in prop::sample::select(vec![2usize, 10, 11, 16, 17, 18]),
        ints in prop::collection::vec(arb_font_int(), 18),
        floats in prop::collection::vec(arb_font_float(), 18),
    ) {
        let mut fields = vec!["Noto Sans".to_owned()];
        for index in 1..count {
            let float_field = matches!(index, 1 | 12 | 13);
            fields.push(if float_field { floats[index].clone() } else { ints[index].clone() });
        }
        let text = fields.join(",");
        if let Some(font) = KdeFont::parse_qfont(&text) {
            prop_assert!((1..=1000).contains(&font.weight), "weight {} from {text:?}", font.weight);
            let _ = font.size_px();
        }
    }
}
