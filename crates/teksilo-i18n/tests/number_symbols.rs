// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for [`NumberSymbols`] — the parse direction that mirrors
//! `NumberFormatter`.
//!
//! The symbols are probed out of ICU's own formatted output, so these
//! assertions double as a check that the probe still finds the parts it
//! expects: a regression there would silently degrade every locale to the
//! C-locale fallback, which `symbols_are_not_silently_the_fallback` guards.

use teksilo_i18n::format::NumberFormatter;
use teksilo_i18n::{I18nConfig, I18nManager, LanguageIdentifier, NumberSymbols, delocalize_number};

fn lid(tag: &str) -> LanguageIdentifier {
    tag.parse().unwrap()
}

fn symbols(tag: &str) -> std::rc::Rc<NumberSymbols> {
    NumberSymbols::for_locale(&lid(tag))
}

#[test]
fn probe_recovers_the_separators_each_locale_actually_uses() {
    let en = symbols("en-US");
    assert_eq!(en.decimal_separator(), ".");
    assert_eq!(en.group_separator(), ",");

    // fr-FR groups with U+202F NARROW NO-BREAK SPACE — the character that
    // makes a naive `f64::from_str` fail on a formatted French number.
    let fr = symbols("fr-FR");
    assert_eq!(fr.decimal_separator(), ",");
    assert_eq!(fr.group_separator(), "\u{202f}");

    // de-DE is the dangerous one: `.` is the *group* separator, so
    // "1.234" is one thousand two hundred thirty-four, not 1.234.
    let de = symbols("de-DE");
    assert_eq!(de.decimal_separator(), ",");
    assert_eq!(de.group_separator(), ".");
}

#[test]
fn probe_recovers_non_ascii_signs() {
    // sv-SE uses U+2212 MINUS SIGN, not ASCII hyphen.
    assert_eq!(symbols("sv-SE").minus_sign(), "\u{2212}");
    // ar-EG prefixes its signs with U+061C ARABIC LETTER MARK.
    assert_eq!(symbols("ar-EG").minus_sign(), "\u{61c}-");
}

#[test]
fn probe_recovers_non_ascii_digits() {
    let ar = symbols("ar-EG");
    assert_eq!(ar.zero_digit(), '٠');
    assert!(ar.has_non_ascii_digits());
    assert!(!symbols("en-US").has_non_ascii_digits());
}

#[test]
fn symbols_are_not_silently_the_fallback() {
    // If the parts probe ever stops finding GROUP/DECIMAL it would fall
    // back to C-locale symbols for every locale, and every other test here
    // that asserts a *specific* separator would still be meaningful — but
    // this one states the intent directly.
    assert_ne!(*symbols("fr-FR"), NumberSymbols::default());
    assert_eq!(*symbols("en-US"), NumberSymbols::default());
}

#[test]
fn delocalize_round_trips_the_formatters_own_output() {
    // The property that matters: whatever `NumberFormatter` renders,
    // `delocalize` must read back. This is why the symbols are probed from
    // the formatter rather than read out of a provider struct.
    for tag in ["en-US", "fr-FR", "de-DE", "sv-SE", "ja-JP", "tr-TR"] {
        teksilo_i18n::thread_local::clear();
        let cfg = I18nConfig::test_only(tag, &[("x", "x")]);
        teksilo_i18n::thread_local::install(I18nManager::from_config(&cfg));

        for value in [1234.5_f64, -1234.5, 0.0, 1_234_567.0, -0.25] {
            let rendered = NumberFormatter::new()
                .fraction_digits(1, 4)
                .format(value)
                .get();
            let back = delocalize_number(&rendered)
                .unwrap_or_else(|| panic!("{tag}: could not de-localize {rendered:?}"));
            let parsed: f64 = back
                .parse()
                .unwrap_or_else(|e| panic!("{tag}: {rendered:?} -> {back:?} does not parse: {e}"));
            assert!(
                (parsed - value).abs() < 1e-9,
                "{tag}: {value} rendered {rendered:?}, read back as {parsed}"
            );
        }
    }
    teksilo_i18n::thread_local::clear();
}

#[test]
fn delocalize_accepts_ascii_input_in_a_non_ascii_digit_locale() {
    // People type on the keyboard they have.
    let ar = symbols("ar-EG");
    assert_eq!(ar.delocalize("١٢٣٤٫٥٦").as_deref(), Some("1234.56"));
    assert_eq!(ar.delocalize("1234.56").as_deref(), Some("1234.56"));
}

#[test]
fn delocalize_accepts_a_plain_space_where_the_locale_groups_with_nbsp() {
    // A hand-typed or pasted ASCII space stands in for U+202F.
    assert_eq!(
        symbols("fr-FR").delocalize("1 234,56").as_deref(),
        Some("1234.56")
    );
    assert_eq!(
        symbols("fr-FR").delocalize("1\u{202f}234,56").as_deref(),
        Some("1234.56")
    );
}

#[test]
fn delocalize_reads_a_dot_as_grouping_where_cldr_says_it_is_grouping() {
    // de-DE: documented, deliberate. "1.234" is 1234, and "1.5" is 15,
    // because that is what a `.` means when this locale writes a number.
    let de = symbols("de-DE");
    assert_eq!(de.delocalize("1.234,56").as_deref(), Some("1234.56"));
    assert_eq!(de.delocalize("1.5").as_deref(), Some("15"));
}

#[test]
fn delocalize_accepts_the_numpad_dot_where_the_locale_does_not_use_it() {
    // fr-FR uses `,` for decimals and U+202F for groups, so a bare `.`
    // is unambiguous and should behave like the decimal separator.
    assert_eq!(symbols("fr-FR").delocalize("12.5").as_deref(), Some("12.5"));
}

#[test]
fn delocalize_rejects_input_that_cannot_be_a_number() {
    let en = symbols("en-US");
    assert_eq!(en.delocalize("12abc"), None);
    assert_eq!(en.delocalize(""), None);
    assert_eq!(en.delocalize("   "), None);
    assert_eq!(en.delocalize("-"), None);
    // Structurally invalid *after* rewriting: fr-FR turns `,` into `.`,
    // and the stray `.` would make a second decimal point.
    assert_eq!(symbols("fr-FR").delocalize("1.234,56"), None);
}

#[test]
fn delocalize_preserves_sign_and_exponent() {
    let fr = symbols("fr-FR");
    assert_eq!(fr.delocalize("-12,5").as_deref(), Some("-12.5"));
    assert_eq!(fr.delocalize("+12,5").as_deref(), Some("+12.5"));
    assert_eq!(fr.delocalize("1,5e3").as_deref(), Some("1.5e3"));
    // sv-SE's U+2212 minus normalizes to ASCII.
    assert_eq!(
        symbols("sv-SE").delocalize("\u{2212}12,5").as_deref(),
        Some("-12.5")
    );
}

#[test]
fn delocalize_does_not_lose_integers_past_f64_exactness() {
    // The reason this layer returns a *string* rather than an f64: the
    // caller's own type decides exactness. A u64 beyond 2^53 survives.
    let en = symbols("en-US");
    let back = en.delocalize("9,007,199,254,740,993").unwrap();
    assert_eq!(back, "9007199254740993");
    assert_eq!(back.parse::<u64>().unwrap(), 9_007_199_254_740_993);
}

#[test]
fn no_manager_falls_back_to_the_c_locale() {
    teksilo_i18n::thread_local::clear();
    assert_eq!(delocalize_number("1,234.5").as_deref(), Some("1234.5"));
}

#[test]
fn localize_is_the_inverse_of_delocalize() {
    for tag in ["en-US", "fr-FR", "de-DE", "sv-SE", "ar-EG", "hi-IN"] {
        let s = symbols(tag);
        for plain in ["0", "1234", "-1234", "1234.56", "-0.25", "9007199254740993"] {
            for grouping in [false, true] {
                let shown = s.localize(plain, grouping);
                assert_eq!(
                    s.delocalize(&shown).as_deref(),
                    Some(plain),
                    "{tag} (grouping={grouping}): {plain:?} rendered {shown:?}"
                );
            }
        }
    }
}

#[test]
fn localize_uses_the_locale_separator_and_digits() {
    assert_eq!(symbols("fr-FR").localize("1234.5", false), "1234,5");
    assert_eq!(symbols("de-DE").localize("1234.5", false), "1234,5");
    assert_eq!(symbols("en-US").localize("1234.5", false), "1234.5");
    assert_eq!(symbols("ar-EG").localize("1234.5", false), "١٢٣٤٫٥");
    // sv-SE's minus is U+2212.
    assert_eq!(symbols("sv-SE").localize("-12.5", false), "\u{2212}12,5");
}

#[test]
fn localize_grouping_follows_the_locales_group_sizes() {
    assert_eq!(symbols("en-US").localize("1234567", true), "1,234,567");
    assert_eq!(symbols("de-DE").localize("1234567", true), "1.234.567");
    assert_eq!(
        symbols("fr-FR").localize("1234567", true),
        "1\u{202f}234\u{202f}567"
    );
    // hi-IN groups in the lakh/crore system: 3 then 2.
    let hi = symbols("hi-IN");
    assert_eq!(hi.primary_group_size(), 3);
    assert_eq!(hi.secondary_group_size(), 2);
    assert_eq!(hi.localize("1234567", true), "12,34,567");
}

#[test]
fn localize_keeps_integers_exact_past_f64() {
    // The reason `localize` takes a string: an f64 round-trip would turn
    // this into ...992.
    let en = symbols("en-US");
    assert_eq!(
        en.localize("9007199254740993", true),
        "9,007,199,254,740,993"
    );
}

#[test]
fn localize_leaves_scientific_notation_alone() {
    assert_eq!(symbols("fr-FR").localize("1.5e3", false), "1.5e3");
}
