// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The editors' own rebindable commands: bold and the rest of
//! [`RichTextEditor`](crate::rich_text::RichTextEditor)'s formatting
//! ([`FormatShortcut`]), comment and duplicate in
//! [`CodeEditor`](crate::CodeEditor) ([`CodeShortcut`]).
//!
//! They work as the standard edit commands do
//! ([`StandardEdit`](teksilo_core::shortcut::StandardEdit)): a global shortcut
//! whose id is also the intent name, and an action on each editor that runs
//! it, so the focused editor answers. Where the standard set is registered in
//! every tree, these exist only while an editor that runs them does: each
//! editable instance declares them with
//! [`register_shortcut_shared`](teksilo_core::build_context::BuildContext::register_shortcut_shared),
//! and the shortcut goes with the last. A read-only viewer declares none. A
//! settings UI therefore lists Bold only in an application that has a
//! rich-text editor, and a user's binding waits in the registry for the next
//! one.
//!
//! An application that binds one of these chords to a global shortcut of its
//! own shares it with the editor's while an editor exists.

use teksilo_core::event::{Key, Modifiers};
use teksilo_core::shortcut::{KeyStroke, Shortcut};
use teksilo_i18n::{LocalizedString, tr_widget};

/// The settings-UI category of every [`FormatShortcut`].
pub const FORMAT_CATEGORY: &str = "format";

/// The settings-UI category of every [`CodeShortcut`].
pub const CODE_CATEGORY: &str = "code";

/// A formatting command of `RichTextEditor`. Its [`id`](Self::id) is both the
/// shortcut id and the intent name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FormatShortcut {
    /// `format.bold`, Ctrl+B (⌘B on macOS).
    Bold,
    /// `format.italic`, Ctrl+I (⌘I on macOS).
    Italic,
    /// `format.underline`, Ctrl+U (⌘U on macOS).
    Underline,
    /// `format.insert_paragraph`, Ctrl+Enter (⌘Enter on macOS): a new
    /// paragraph even inside a table cell, where Enter moves to the cell
    /// below.
    InsertParagraph,
}

impl FormatShortcut {
    /// Every formatting command, in the order a settings UI lists them.
    pub const ALL: [FormatShortcut; 4] = [
        FormatShortcut::Bold,
        FormatShortcut::Italic,
        FormatShortcut::Underline,
        FormatShortcut::InsertParagraph,
    ];

    /// The shortcut id, which is also the intent name.
    pub const fn id(self) -> &'static str {
        match self {
            FormatShortcut::Bold => "format.bold",
            FormatShortcut::Italic => "format.italic",
            FormatShortcut::Underline => "format.underline",
            FormatShortcut::InsertParagraph => "format.insert_paragraph",
        }
    }

    /// The command whose [`id`](Self::id) is `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    /// The shortcut an editor declares for this command: its default chord, a
    /// name in the active language, and the [`FORMAT_CATEGORY`] category.
    pub fn shortcut(self) -> Shortcut {
        let (name, key) = match self {
            FormatShortcut::Bold => (tr_widget!(shortcut_format_bold()), Key::B),
            FormatShortcut::Italic => (tr_widget!(shortcut_format_italic()), Key::I),
            FormatShortcut::Underline => (tr_widget!(shortcut_format_underline()), Key::U),
            FormatShortcut::InsertParagraph => {
                (tr_widget!(shortcut_format_insert_paragraph()), Key::Enter)
            }
        };
        described(
            self.id(),
            name,
            FORMAT_CATEGORY,
            tr_widget!(shortcut_category_format()),
        )
        .primary(KeyStroke::ctrl(key))
        .build()
    }
}

/// A command of `CodeEditor`. Its [`id`](Self::id) is both the shortcut id and
/// the intent name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CodeShortcut {
    /// `code.toggle_comment`, Ctrl+/ (⌘/ on macOS), and Ctrl+Shift+/ for the
    /// layouts that type `/` with Shift (AZERTY, German).
    ToggleComment,
    /// `code.duplicate`, Ctrl+D (⌘D on macOS): the caret's line, or the
    /// selection, below itself.
    Duplicate,
}

impl CodeShortcut {
    /// Every code command, in the order a settings UI lists them.
    pub const ALL: [CodeShortcut; 2] = [CodeShortcut::ToggleComment, CodeShortcut::Duplicate];

    /// The shortcut id, which is also the intent name.
    pub const fn id(self) -> &'static str {
        match self {
            CodeShortcut::ToggleComment => "code.toggle_comment",
            CodeShortcut::Duplicate => "code.duplicate",
        }
    }

    /// The command whose [`id`](Self::id) is `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    /// The shortcut an editor declares for this command: its default chords, a
    /// name in the active language, and the [`CODE_CATEGORY`] category.
    pub fn shortcut(self) -> Shortcut {
        let category = || tr_widget!(shortcut_category_code());
        match self {
            CodeShortcut::ToggleComment => described(
                self.id(),
                tr_widget!(shortcut_code_toggle_comment()),
                CODE_CATEGORY,
                category(),
            )
            .primary(KeyStroke::ctrl(Key::Character('/')))
            // A layout that types `/` with Shift reports the chord with Shift
            // held; on one that does not, Shift+/ types `?` and this never
            // matches.
            .secondary(KeyStroke::new(
                Key::Character('/'),
                Modifiers::CTRL | Modifiers::SHIFT,
            ))
            .build(),
            CodeShortcut::Duplicate => described(
                self.id(),
                tr_widget!(shortcut_code_duplicate()),
                CODE_CATEGORY,
                category(),
            )
            .primary(KeyStroke::ctrl(Key::D))
            .build(),
        }
    }
}

fn described(
    id: &'static str,
    name: LocalizedString,
    category: &'static str,
    category_label: LocalizedString,
) -> teksilo_core::shortcut::ShortcutBuilder {
    Shortcut::new(id)
        .name(name)
        .category(category)
        .category_label(category_label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for command in FormatShortcut::ALL {
            assert_eq!(FormatShortcut::from_id(command.id()), Some(command));
            assert_eq!(command.shortcut().id, command.id());
        }
        for command in CodeShortcut::ALL {
            assert_eq!(CodeShortcut::from_id(command.id()), Some(command));
            assert_eq!(command.shortcut().id, command.id());
        }
        assert_eq!(FormatShortcut::from_id("code.duplicate"), None);
    }
}
