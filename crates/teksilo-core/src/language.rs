// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The language of a piece of content, as a BCP 47 tag.
//!
//! A [`LanguageTag`] says what language some text is written in: `"ja"`,
//! `"ar-EG"`, `"sr-Latn"`. It is declared on a subtree with
//! [`WidgetBuilder::content_language`](crate::widget_builder::WidgetBuilder::content_language),
//! the equivalent of HTML's `lang` attribute, and read back with
//! `content_language()` on the layout, paint and event contexts.
//!
//! A tag is checked and written in its canonical case when it is parsed
//! (`en-us` becomes `en-US`, `ZH-hant` becomes `zh-Hant`). The platform
//! accessibility layers need that: Windows turns the tag into a locale
//! identifier and drops it when that fails, and Orca splits it at its first
//! `-` into a language and a dialect.
//!
//! The language does not decide a text's direction.
//! [`LanguageTag::likely_direction`] is only the direction to fall back on
//! when the text itself has no strongly directional character (an empty
//! field, a field of digits). A language does not fix its script: Azerbaijani
//! is written in Latin (`az-Latn`) and in Arabic (`az-Arab`) script.

use std::sync::Arc;

use crate::environment::LayoutDirection;

/// Scripts written right to left. A script subtag decides the direction on
/// its own, whatever the language.
const RTL_SCRIPTS: &[&str] = &[
    "Arab", "Hebr", "Syrc", "Thaa", "Nkoo", "Samr", "Mand", "Mend", "Adlm",
];

/// Languages whose usual script is written right to left, for a tag with no
/// script subtag.
///
/// `ku` (Kurdish, which usually means Kurmanji) is not here: Kurmanji is
/// written in Latin script. Sorani, written in Arabic script, has its own
/// code, `ckb`.
const RTL_LANGUAGES: &[&str] = &["ar", "fa", "he", "ur", "yi", "ps", "sd", "dv", "ckb", "ug"];

/// A BCP 47 language tag, checked and in canonical case.
///
/// Holds the language identifier part of a tag: a language subtag, then an
/// optional script, region and variants (`"de-CH-1996"`). Extensions and
/// private-use subtags (`"ja-JP-u-ca-japanese"`, `"x-klingon"`) are refused;
/// they say nothing more about which language the text is in.
///
/// Cheap to clone: the string is shared.
///
/// ```
/// use teksilo_core::language::LanguageTag;
/// use teksilo_core::LayoutDirection;
///
/// let tag = LanguageTag::parse("ar-eg").unwrap();
/// assert_eq!(tag.as_str(), "ar-EG");
/// assert_eq!(tag.likely_direction(), LayoutDirection::RightToLeft);
/// assert!(LanguageTag::parse("not a tag").is_err());
/// ```
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LanguageTag(Arc<str>);

/// Why a string is not a [`LanguageTag`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{tag}` is not a BCP 47 language tag")]
pub struct LanguageTagError {
    /// The string that was refused.
    pub tag: String,
}

impl LanguageTag {
    /// Parse a BCP 47 language tag and write it in canonical case.
    ///
    /// `_` is accepted as a separator and written as `-`, so the locale names
    /// spelling dictionaries and POSIX use (`"en_US"`) are accepted.
    pub fn parse(tag: &str) -> Result<Self, LanguageTagError> {
        let error = || LanguageTagError {
            tag: tag.to_owned(),
        };
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            return Err(error());
        }
        let normalized = trimmed.replace('_', "-");
        let id =
            icu_locale_core::LanguageIdentifier::try_from_str(&normalized).map_err(|_| error())?;
        Ok(Self(Arc::from(id.to_string())))
    }

    /// The tag in canonical case, as AccessKit and the platforms receive it.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The language subtag alone: `"ja"` for `"ja-JP"`, `"ckb"` for
    /// `"ckb-IQ"`.
    pub fn language(&self) -> &str {
        self.0.split('-').next().unwrap_or(&self.0)
    }

    /// The script subtag, when the tag has one: `Some("Latn")` for
    /// `"sr-Latn-RS"`.
    pub fn script(&self) -> Option<&str> {
        // In canonical form a script subtag is the only four-letter subtag
        // that starts with an upper-case letter.
        self.0.split('-').skip(1).find(|subtag| {
            subtag.len() == 4
                && subtag.starts_with(|c: char| c.is_ascii_uppercase())
                && subtag.chars().all(|c| c.is_ascii_alphabetic())
        })
    }

    /// The direction text in this language is usually written in.
    ///
    /// The script subtag decides when there is one (`"az-Arab"` is right to
    /// left, `"az-Latn"` left to right). Without one, the language does.
    ///
    /// Only a fallback: a text input whose content has a strongly directional
    /// character follows the content, never this.
    pub fn likely_direction(&self) -> LayoutDirection {
        if let Some(script) = self.script() {
            return if RTL_SCRIPTS.contains(&script) {
                LayoutDirection::RightToLeft
            } else {
                LayoutDirection::LeftToRight
            };
        }
        if RTL_LANGUAGES.contains(&self.language()) {
            LayoutDirection::RightToLeft
        } else {
            LayoutDirection::LeftToRight
        }
    }
}

impl std::fmt::Debug for LanguageTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LanguageTag({:?})", &*self.0)
    }
}

impl std::fmt::Display for LanguageTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for LanguageTag {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for LanguageTag {
    type Err = LanguageTagError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<&str> for LanguageTag {
    type Error = LanguageTagError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::parse(s)
    }
}

impl TryFrom<String> for LanguageTag {
    type Error = LanguageTagError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<LanguageTag> for String {
    fn from(tag: LanguageTag) -> Self {
        tag.0.to_string()
    }
}

/// A tag declared outright: `.content_language(tag)`.
impl From<LanguageTag> for crate::signal::Prop<Option<LanguageTag>> {
    fn from(tag: LanguageTag) -> Self {
        crate::signal::Prop::Static(Some(tag))
    }
}

/// A tag that follows a signal and is always present:
/// `.content_language(target_language)`.
impl From<crate::signal::Signal<LanguageTag>> for crate::signal::Prop<Option<LanguageTag>> {
    fn from(signal: crate::signal::Signal<LanguageTag>) -> Self {
        crate::signal::Prop::Bound(signal.map(|tag| Some(tag.clone())))
    }
}

impl serde::Serialize for LanguageTag {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for LanguageTag {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &str) -> LanguageTag {
        LanguageTag::parse(s).unwrap()
    }

    #[test]
    fn parsing_writes_the_canonical_case() {
        assert_eq!(tag("en-us").as_str(), "en-US");
        assert_eq!(tag("ZH-hant-tw").as_str(), "zh-Hant-TW");
        assert_eq!(tag("JA").as_str(), "ja");
        assert_eq!(tag("sr-latn").as_str(), "sr-Latn");
    }

    #[test]
    fn an_underscore_is_read_as_a_hyphen() {
        assert_eq!(tag("en_US").as_str(), "en-US");
        assert_eq!(tag("fr_ca").as_str(), "fr-CA");
    }

    #[test]
    fn surrounding_whitespace_is_dropped() {
        assert_eq!(tag("  de-CH ").as_str(), "de-CH");
    }

    #[test]
    fn what_is_not_a_language_identifier_is_refused() {
        for bad in [
            "",
            "   ",
            "not a tag",
            "e",
            "toolonglanguage",
            "en--US",
            "ja-JP-u-ca-japanese",
            "x-klingon",
        ] {
            let err = LanguageTag::parse(bad).unwrap_err();
            assert_eq!(err.tag, bad, "the error names what was refused");
        }
    }

    #[test]
    fn variants_are_kept() {
        assert_eq!(tag("de-ch-1996").as_str(), "de-CH-1996");
    }

    #[test]
    fn subtag_accessors() {
        let t = tag("sr-latn-rs");
        assert_eq!(t.language(), "sr");
        assert_eq!(t.script(), Some("Latn"));
        assert_eq!(tag("ja-JP").script(), None);
        assert_eq!(tag("ckb").language(), "ckb");
    }

    #[test]
    fn direction_from_the_language() {
        for rtl in [
            "ar", "ar-SA", "he-IL", "fa", "ur", "yi", "ps", "sd", "dv", "ckb", "ug",
        ] {
            assert_eq!(
                tag(rtl).likely_direction(),
                LayoutDirection::RightToLeft,
                "{rtl}"
            );
        }
        for ltr in ["en-US", "fr-FR", "ja", "zh-Hans", "ru", "und"] {
            assert_eq!(
                tag(ltr).likely_direction(),
                LayoutDirection::LeftToRight,
                "{ltr}"
            );
        }
    }

    #[test]
    fn kurdish_without_a_script_is_left_to_right() {
        // Kurmanji, the usual meaning of `ku`, is written in Latin script.
        assert_eq!(tag("ku").likely_direction(), LayoutDirection::LeftToRight);
        assert_eq!(
            tag("ku-Arab").likely_direction(),
            LayoutDirection::RightToLeft
        );
    }

    #[test]
    fn the_script_decides_over_the_language() {
        assert_eq!(
            tag("az-Arab").likely_direction(),
            LayoutDirection::RightToLeft
        );
        assert_eq!(
            tag("az-Latn").likely_direction(),
            LayoutDirection::LeftToRight
        );
        // A right-to-left language written in a left-to-right script.
        assert_eq!(
            tag("ar-Latn").likely_direction(),
            LayoutDirection::LeftToRight
        );
        assert_eq!(
            tag("pa-Arab").likely_direction(),
            LayoutDirection::RightToLeft
        );
        assert_eq!(
            tag("sd-Deva").likely_direction(),
            LayoutDirection::LeftToRight
        );
    }

    #[test]
    fn serde_round_trips_through_the_string_and_checks_it() {
        let json = serde_json::to_string(&tag("pt-br")).unwrap();
        assert_eq!(json, "\"pt-BR\"");
        let back: LanguageTag = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tag("pt-BR"));
        assert!(serde_json::from_str::<LanguageTag>("\"not a tag\"").is_err());
    }

    #[test]
    fn into_prop() {
        use crate::signal::{Prop, Signal};
        let p: Prop<Option<LanguageTag>> = tag("ja").into();
        assert_eq!(p.get(), Some(tag("ja")));
        let s = Signal::new(tag("ar"));
        let p: Prop<Option<LanguageTag>> = s.clone().into();
        assert_eq!(p.get(), Some(tag("ar")));
        s.set(tag("he"));
        assert_eq!(p.get(), Some(tag("he")));
    }
}
