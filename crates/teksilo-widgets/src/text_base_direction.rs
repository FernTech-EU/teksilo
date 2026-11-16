// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The base direction of a text a user types: the direction its paragraph
//! reads in, which decides where the text starts, where an empty field puts
//! its caret, and which way a screen reader is told it runs.
//!
//! A text input resolves it, every time it lays out, in this order:
//!
//! 1. an explicit [`TextBaseDirection::LeftToRight`] or
//!    [`TextBaseDirection::RightToLeft`], used as given;
//! 2. with [`TextBaseDirection::Auto`] (the default), the first strongly
//!    directional character of the text (UAX #9, rules P2 and P3): Arabic
//!    text reads right to left, Latin text left to right, whatever the
//!    window's direction;
//! 3. when the text has none (it is empty, or only digits and punctuation),
//!    the [`content_language`](teksilo_core::widget_builder::WidgetBuilder::content_language)
//!    declared on or above the field, through
//!    [`LanguageTag::likely_direction`](teksilo_core::language::LanguageTag::likely_direction);
//! 4. with no language declared, the window's layout direction.
//!
//! So an empty field declared Arabic opens with its caret on the right, and a
//! field declared Japanese that holds Arabic text still reads right to left:
//! the language never decides against the text. This is HTML's `dir="auto"`,
//! falling back on the language before the surrounding direction.

use teksilo_core::environment::LayoutDirection;
use teksilo_core::language::LanguageTag;

/// Which way a typed text's paragraph reads. See the [module](self) for how
/// a text input resolves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextBaseDirection {
    /// The text decides: its first strongly directional character, else the
    /// declared language's usual direction, else the window's.
    #[default]
    Auto,
    /// Left to right, whatever the text.
    LeftToRight,
    /// Right to left, whatever the text.
    RightToLeft,
}

impl TextBaseDirection {
    /// The direction a paragraph of `text` reads in: see the [module](self).
    /// `language` is the content language declared on or above the field,
    /// `window` the window's layout direction.
    pub fn resolve(
        self,
        text: &str,
        language: Option<&LanguageTag>,
        window: LayoutDirection,
    ) -> LayoutDirection {
        from_engine(teksilo_text::resolve_paragraph_direction(
            text,
            self.for_engine(),
            to_engine(neutral_direction(language, window)),
        ))
    }

    /// The direction the editing engine is given for blocks that store none:
    /// `Auto` lets each block's text decide.
    pub(crate) fn for_engine(self) -> teksilo_text::TextDirection {
        match self {
            Self::Auto => teksilo_text::TextDirection::Auto,
            Self::LeftToRight => teksilo_text::TextDirection::LeftToRight,
            Self::RightToLeft => teksilo_text::TextDirection::RightToLeft,
        }
    }
}

/// The direction a text with no strongly directional character takes: the
/// declared language's usual direction, else the window's.
pub(crate) fn neutral_direction(
    language: Option<&LanguageTag>,
    window: LayoutDirection,
) -> LayoutDirection {
    language.map_or(window, LanguageTag::likely_direction)
}

pub(crate) fn to_engine(direction: LayoutDirection) -> teksilo_text::TextDirection {
    match direction {
        LayoutDirection::LeftToRight => teksilo_text::TextDirection::LeftToRight,
        LayoutDirection::RightToLeft => teksilo_text::TextDirection::RightToLeft,
    }
}

pub(crate) fn from_engine(direction: teksilo_text::TextDirection) -> LayoutDirection {
    match direction {
        teksilo_text::TextDirection::RightToLeft => LayoutDirection::RightToLeft,
        _ => LayoutDirection::LeftToRight,
    }
}

/// The AccessKit direction of a layout direction.
pub(crate) fn to_accesskit(direction: LayoutDirection) -> teksilo_core::accesskit::TextDirection {
    match direction {
        LayoutDirection::LeftToRight => teksilo_core::accesskit::TextDirection::LeftToRight,
        LayoutDirection::RightToLeft => teksilo_core::accesskit::TextDirection::RightToLeft,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LTR: LayoutDirection = LayoutDirection::LeftToRight;
    const RTL: LayoutDirection = LayoutDirection::RightToLeft;

    fn tag(s: &str) -> LanguageTag {
        LanguageTag::parse(s).unwrap()
    }

    #[test]
    fn auto_follows_the_text_whatever_the_language_and_the_window() {
        let auto = TextBaseDirection::Auto;
        assert_eq!(auto.resolve("مرحبا", None, LTR), RTL);
        assert_eq!(auto.resolve("مرحبا", Some(&tag("ja")), LTR), RTL);
        assert_eq!(auto.resolve("hello", Some(&tag("ar")), RTL), LTR);
    }

    #[test]
    fn with_no_strong_character_the_language_then_the_window_decides() {
        let auto = TextBaseDirection::Auto;
        assert_eq!(auto.resolve("", Some(&tag("ar")), LTR), RTL);
        assert_eq!(auto.resolve("123", Some(&tag("he")), LTR), RTL);
        assert_eq!(auto.resolve("", Some(&tag("ja")), RTL), LTR);
        assert_eq!(auto.resolve("", None, RTL), RTL);
        assert_eq!(auto.resolve("", None, LTR), LTR);
    }

    #[test]
    fn an_explicit_direction_is_used_as_given() {
        assert_eq!(
            TextBaseDirection::LeftToRight.resolve("مرحبا", Some(&tag("ar")), RTL),
            LTR
        );
        assert_eq!(
            TextBaseDirection::RightToLeft.resolve("hello", None, LTR),
            RTL
        );
    }
}
