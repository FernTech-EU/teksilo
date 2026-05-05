//! Layout direction derivation from a `LanguageIdentifier`.

use fern_core::environment::LayoutDirection;
use unic_langid::LanguageIdentifier;

const RTL_SCRIPTS: &[&str] = &[
    "Arab", "Hebr", "Syrc", "Thaa", "Nkoo", "Samr", "Mand", "Mend", "Adlm",
];

const RTL_LANGUAGES: &[&str] = &[
    "ar", "fa", "he", "ur", "yi", "ps", "sd", "ku", "dv", "ckb", "ug",
];

/// Derive layout direction from a locale.
///
/// Checks the script subtag first (`ar-Arab-SA` → RTL via `Arab`); if no
/// script subtag is present, falls back to a hardcoded list of languages
/// that always render RTL (`ar-SA` → RTL via `ar`).
pub fn rtl_from_locale(locale: &LanguageIdentifier) -> LayoutDirection {
    if let Some(script) = locale.script.as_ref() {
        let s: &str = script.into();
        if RTL_SCRIPTS.contains(&s) {
            return LayoutDirection::RightToLeft;
        }
    }
    let lang: &str = locale.language.as_str();
    if RTL_LANGUAGES.contains(&lang) {
        return LayoutDirection::RightToLeft;
    }
    LayoutDirection::LeftToRight
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
    fn ltr_when_language_uses_latin_script() {
        // Maltese uses Latin script even though it's a Semitic language.
        assert_eq!(
            rtl_from_locale(&parse("mt-MT")),
            LayoutDirection::LeftToRight
        );
    }
}
