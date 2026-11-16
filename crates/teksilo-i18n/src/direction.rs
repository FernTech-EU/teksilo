// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Layout direction derivation from a `LanguageIdentifier`.

use teksilo_core::environment::LayoutDirection;
use teksilo_core::language::LanguageTag;
use unic_langid::LanguageIdentifier;

/// Derive layout direction from a locale.
///
/// The script subtag decides when there is one (`ar-Arab-SA` is right to
/// left, `az-Latn` left to right); without one, the language does (`ar-SA`
/// is right to left). The table is
/// [`LanguageTag::likely_direction`]'s, the one a text input falls back on,
/// so the interface and the fields agree.
pub fn rtl_from_locale(locale: &LanguageIdentifier) -> LayoutDirection {
    LanguageTag::parse(&locale.to_string())
        .map(|tag| tag.likely_direction())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> LanguageIdentifier {
        s.parse().unwrap()
    }

    #[test]
    fn ltr_default() {
        assert_eq!(
            rtl_from_locale(&parse("en-US")),
            LayoutDirection::LeftToRight
        );
        assert_eq!(
            rtl_from_locale(&parse("fr-FR")),
            LayoutDirection::LeftToRight
        );
        assert_eq!(
            rtl_from_locale(&parse("zh-Hans")),
            LayoutDirection::LeftToRight
        );
    }

    #[test]
    fn rtl_via_language() {
        assert_eq!(
            rtl_from_locale(&parse("ar-SA")),
            LayoutDirection::RightToLeft
        );
        assert_eq!(
            rtl_from_locale(&parse("he-IL")),
            LayoutDirection::RightToLeft
        );
        assert_eq!(
            rtl_from_locale(&parse("fa-IR")),
            LayoutDirection::RightToLeft
        );
        assert_eq!(rtl_from_locale(&parse("ur")), LayoutDirection::RightToLeft);
    }

    #[test]
    fn rtl_via_script_subtag() {
        assert_eq!(
            rtl_from_locale(&parse("ar-Arab")),
            LayoutDirection::RightToLeft
        );
        assert_eq!(
            rtl_from_locale(&parse("ks-Arab-IN")),
            LayoutDirection::RightToLeft
        );
    }

    #[test]
    fn the_script_decides_over_the_language() {
        assert_eq!(
            rtl_from_locale(&parse("az-Arab")),
            LayoutDirection::RightToLeft
        );
        // Arabic written in Latin script reads left to right.
        assert_eq!(
            rtl_from_locale(&parse("ar-Latn")),
            LayoutDirection::LeftToRight
        );
    }

    #[test]
    fn kurdish_without_a_script_is_left_to_right() {
        // Kurmanji is written in Latin script; Sorani (`ckb`) in Arabic.
        assert_eq!(rtl_from_locale(&parse("ku")), LayoutDirection::LeftToRight);
        assert_eq!(
            rtl_from_locale(&parse("ckb-IQ")),
            LayoutDirection::RightToLeft
        );
    }

    #[test]
    fn ltr_when_language_uses_latin_script() {
        // Maltese uses Latin script even though it's a Semitic language.
        assert_eq!(
            rtl_from_locale(&parse("mt-MT")),
            LayoutDirection::LeftToRight
        );
    }
}
