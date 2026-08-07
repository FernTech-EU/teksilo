// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Locale display names — language *endonyms*.
//!
//! An endonym is a language's name written *in that language itself*:
//! "français" (not "French"), "Deutsch" (not "German"), "العربية" (not
//! "Arabic"). The [`LanguageSwitcher`](../../teksilo_widgets/struct.LanguageSwitcher.html)
//! widget uses this so every entry in a language picker is legible to a
//! speaker of that language.
//!
//! Backed by ICU4X CLDR display-name data via `icu_experimental`'s
//! `displaynames` module. The data is **baked into the binary** (the
//! `compiled_data` feature), so the display-name tables add to binary size —
//! the trade for offline, dependency-free-at-runtime correctness across every
//! CLDR locale.

use std::str::FromStr;

use icu_experimental::displaynames::DisplayNamesOptions;
use icu_experimental::displaynames::multi::LanguageDisplayNames;
use icu_locale_core::Locale as IcuLocale;
use unic_langid::LanguageIdentifier;

/// The language's own name (endonym) for `loc`.
///
/// The display locale is set to the target locale itself, so the returned
/// string is the language's self-name: `language_endonym("fr-FR")` →
/// `Some("français")`, `language_endonym("ar-SA")` → `Some("العربية")`.
///
/// Returns `None` when ICU has no display name for the language subtag (e.g.
/// a private-use or otherwise unknown tag) — callers should fall back to the
/// raw BCP-47 tag.
pub fn language_endonym(loc: &LanguageIdentifier) -> Option<String> {
    let icu_loc = IcuLocale::from_str(&loc.to_string()).ok()?;
    let language = icu_loc.id.language;
    // Display *in* the target locale → self-name → endonym.
    let names =
        LanguageDisplayNames::try_new(icu_loc.into(), DisplayNamesOptions::default()).ok()?;
    names.of(language).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(tag: &str) -> LanguageIdentifier {
        tag.parse().unwrap()
    }

    #[test]
    fn french_endonym_is_self_named() {
        // CLDR spells it lowercase "français".
        assert_eq!(
            language_endonym(&lang("fr-FR")).as_deref(),
            Some("français")
        );
    }

    #[test]
    fn german_endonym_is_self_named() {
        assert_eq!(language_endonym(&lang("de-DE")).as_deref(), Some("Deutsch"));
    }

    #[test]
    fn arabic_endonym_is_self_named() {
        // RTL script — the data is right-to-left; just assert it resolves and
        // is non-empty (avoid pasting the exact glyph sequence into source).
        let endonym = language_endonym(&lang("ar-SA"));
        assert!(endonym.is_some(), "expected an Arabic endonym");
        assert!(!endonym.unwrap().is_empty());
    }

    #[test]
    fn language_only_tag_resolves() {
        assert_eq!(language_endonym(&lang("en")).as_deref(), Some("English"));
    }
}
