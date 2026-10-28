// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`EditSource`]: the input route a piece of text reached a text surface by.
//!
//! Shared by every surface that reports its arrivals (`RichTextEditor` and
//! `TextInputField` with the composites built on it), which is why it lives at
//! the crate root rather than in either widget's module.

use teksilo_core::event::ImeOrigin;

/// How a piece of text reached the document: the **channel**, not the author.
///
/// Deliberately framework-generic, and deliberately small. These are the routes
/// a toolkit can actually observe: which input path the characters came down.
/// What that *means* is the application's to decide, and every application will
/// decide differently. A writing tool cares that dictation is not typing, a code
/// editor cares that a snippet is not either, and a form cares about none of it.
/// Teksilo says what it saw; it does not interpret.
///
/// ⚠ **Not evidence of who wrote anything.** Text typed one character at a time
/// was typed one character at a time, and that is the entire claim. Anything
/// further (who, or whether a person at all) is an inference this cannot make
/// and no consumer of it should pretend to.
///
/// Exhaustive on purpose: an application accounting for where its text came
/// from wants a compile error when a new channel appears, not a wildcard arm
/// that quietly counts it as something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditSource {
    /// Typed, one key at a time.
    Keyboard,
    /// The settled result of an IME composition (CJK/Kana candidate selection,
    /// a dead-key accent), and text the platform's input system inserts outside
    /// a key press: the emoji viewer or panel and AutoFill (they arrive as
    /// [`ImeOrigin::Insertion`]). Separate from [`Self::Keyboard`] because the
    /// characters that land are not the keys that were pressed.
    Ime,
    /// Speech the platform's dictation recognised, through its text input
    /// system (macOS Dictation, Windows voice typing).
    ///
    /// Separate from [`Self::Accessibility`], which is an assistive technology
    /// writing through AccessKit; this is the platform's own input system.
    Dictation,
    /// Text a system writing assistant produced to replace the user's (macOS
    /// Writing Tools).
    ///
    /// This is machine-produced text, which an application accounting for
    /// provenance will want apart from everything else.
    WritingTools,
    /// Pasted, as plain text or as HTML.
    Clipboard,
    /// Written by an assistive technology through AccessKit's `SetValue` or
    /// `ReplaceSelectedText`: a braille display, an assistive technology's own
    /// dictation.
    ///
    /// **Never folded into [`Self::Keyboard`].** For some people this *is*
    /// typing, and a toolkit that reported it as something else, or as nothing,
    /// would be quietly erasing how they work.
    Accessibility,
    /// Inserted by the application itself rather than by anything the person at
    /// the keyboard did: a template, a substitution, a completion.
    Programmatic,
}

impl EditSource {
    /// The source an input-system commit from `origin` is reported under.
    ///
    /// A composition and an insertion are both [`Self::Ime`]: neither is a key
    /// pressed, and the platform does not always tell them apart. An origin
    /// this was not written for is [`Self::Ime`] too, which is what every
    /// commit was reported as before origins existed.
    pub(crate) fn from_ime_origin(origin: ImeOrigin) -> Self {
        match origin {
            ImeOrigin::Composition | ImeOrigin::Insertion => Self::Ime,
            ImeOrigin::Dictation => Self::Dictation,
            ImeOrigin::WritingTools => Self::WritingTools,
            _ => Self::Ime,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_origin_maps_to_its_source() {
        assert_eq!(
            EditSource::from_ime_origin(ImeOrigin::Composition),
            EditSource::Ime
        );
        assert_eq!(
            EditSource::from_ime_origin(ImeOrigin::Insertion),
            EditSource::Ime,
            "the emoji viewer and AutoFill are not keystrokes"
        );
        assert_eq!(
            EditSource::from_ime_origin(ImeOrigin::Dictation),
            EditSource::Dictation
        );
        assert_eq!(
            EditSource::from_ime_origin(ImeOrigin::WritingTools),
            EditSource::WritingTools
        );
    }
}
