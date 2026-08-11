// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The code editor's command vocabulary and construction presets.
//!
//! The policy *machinery* — the command filter, the caret behaviour, the AT
//! role, the clipboard surface, and the `PolicyBundle` that ties the four
//! together — is shared with every other text surface and lives in the
//! crate-internal `common::editor_runtime`. What lives here is only what is
//! genuinely code-specific: the list of commands.
//!
//! That list is deliberately *not* the rich text editor's. `EditCommandKind`
//! knows about tables, lists, blockquotes, and bold — none of which mean
//! anything in a source file, and all of which would be reachable keystrokes if
//! the vocabulary were reused. Conversely nothing here knows about any
//! particular language: indentation width, comment tokens, and bracket pairs
//! are injected configuration, so `ToggleLineComment` is a command and `//` is
//! not.

use crate::common::editor_runtime::{
    AccessibilityRole, CaretPolicy, ClipboardPolicy, CommandFilter, EditorCommand, PolicyBundle,
};

/// Commands the code editor's keyboard layer may emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeCommand {
    // --- Text editing ---
    /// Insert typed text at every caret.
    InsertChar,
    /// Break the line, carrying the previous line's indentation when
    /// auto-indent is on.
    InsertNewline,
    DeletePrev,
    DeleteNext,
    DeleteWordLeft,
    DeleteWordRight,

    // --- Code structure (all driven by injected configuration) ---
    /// Tab with a selection, or with `use_soft_tabs`: indent by one level.
    IndentLines,
    /// Shift+Tab: remove one indent level from every touched line.
    DedentLines,
    /// Comment or uncomment the touched lines with the configured token.
    /// A no-op when no line-comment token is configured — the editor knows
    /// the *operation*, the app supplies the language.
    ToggleLineComment,
    /// Duplicate the caret's line (or the selection) below itself.
    DuplicateSelection,
    MoveLineUp,
    MoveLineDown,

    // --- Multi-caret ---
    /// Add a caret on the line above / below the topmost / bottommost one.
    AddCaretAbove,
    AddCaretBelow,
    /// Collapse back to a single caret (Escape).
    ClearExtraCarets,

    // --- History ---
    Undo,
    Redo,

    // --- Navigation (never mutates, always accepted) ---
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    MoveWordLeft,
    MoveWordRight,
    /// Home. Toggles between the first non-whitespace character and column 0 —
    /// the near-universal code-editor behaviour.
    MoveLineStart,
    MoveLineEnd,
    MoveDocStart,
    MoveDocEnd,
    PageUp,
    PageDown,
    SelectLeft,
    SelectRight,
    SelectUp,
    SelectDown,
    SelectWordLeft,
    SelectWordRight,
    SelectLineStart,
    SelectLineEnd,
    SelectDocStart,
    SelectDocEnd,
    SelectAll,

    // --- Clipboard ---
    Copy,
    Cut,
    Paste,
}

impl EditorCommand for CodeCommand {
    /// True for commands that can take text away.
    ///
    /// `CommandFilter::ForwardOnly` is a prose-drafting mode and no code
    /// surface uses it today, but the classification is exhaustive anyway so
    /// the two vocabularies cannot drift: a command added here has to answer
    /// the question rather than inherit a permissive default. The line-motion
    /// commands count as regressive because each removes a line from where it
    /// was, and `DuplicateSelection` does not because it only adds.
    fn is_regressive(&self) -> bool {
        matches!(
            self,
            Self::DeletePrev
                | Self::DeleteNext
                | Self::DeleteWordLeft
                | Self::DeleteWordRight
                | Self::Cut
                | Self::Undo
                | Self::Redo
                | Self::MoveLineUp
                | Self::MoveLineDown
        )
    }

    fn mutates_document(&self) -> bool {
        matches!(
            self,
            Self::InsertChar
                | Self::InsertNewline
                | Self::DeletePrev
                | Self::DeleteNext
                | Self::DeleteWordLeft
                | Self::DeleteWordRight
                | Self::IndentLines
                | Self::DedentLines
                | Self::ToggleLineComment
                | Self::DuplicateSelection
                | Self::MoveLineUp
                | Self::MoveLineDown
                | Self::Undo
                | Self::Redo
                | Self::Cut
                | Self::Paste
        )
    }
}

/// The editable preset, shared by `CodeEditor::new` and
/// `PlainTextEditor::new`: every command accepted, blinking caret,
/// `MultilineTextInput` role, full clipboard.
pub const CODE_EDITOR_PRESET: PolicyBundle = PolicyBundle {
    command_filter: CommandFilter::All,
    caret_policy: CaretPolicy::Blinking,
    access_role: AccessibilityRole::Editor,
    clipboard_policy: ClipboardPolicy::Full,
};

/// The read-only preset for `CodeEditor::read_only` / `PlainTextEditor::read_only`
/// and for a streaming log view.
///
/// Navigation and copy only, `Role::Document`, no caret.
///
/// `Document` rather than `Role::Code` or `Role::Log` is a correctness
/// constraint, not taste: `accesskit_consumer::Node::supports_text_ranges()`
/// admits only text inputs plus `Label | Document | Terminal`. A viewer
/// reporting `Code` or `Log` would render its text to a screen reader once and
/// then never report the caret or selection moving through it — the reader
/// could not navigate what it had just announced. A log that wants its new
/// lines spoken pairs this with an explicit `Live`, which is an independent
/// property rather than something a role implies.
pub const CODE_READ_ONLY_PRESET: PolicyBundle = PolicyBundle {
    command_filter: CommandFilter::ReadOnly,
    caret_policy: CaretPolicy::Hidden,
    access_role: AccessibilityRole::Document,
    clipboard_policy: ClipboardPolicy::CopyAndSelectAllOnly,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_rejects_every_code_mutation() {
        let f = CommandFilter::ReadOnly;
        for cmd in [
            CodeCommand::InsertChar,
            CodeCommand::InsertNewline,
            CodeCommand::DeletePrev,
            CodeCommand::IndentLines,
            CodeCommand::DedentLines,
            CodeCommand::ToggleLineComment,
            CodeCommand::DuplicateSelection,
            CodeCommand::MoveLineUp,
            CodeCommand::MoveLineDown,
            CodeCommand::Undo,
            CodeCommand::Redo,
            CodeCommand::Cut,
            CodeCommand::Paste,
        ] {
            assert!(!f.accepts(cmd), "read-only must reject {cmd:?}");
        }
    }

    #[test]
    fn read_only_still_navigates_selects_and_copies() {
        let f = CommandFilter::ReadOnly;
        for cmd in [
            CodeCommand::MoveLeft,
            CodeCommand::MoveLineStart,
            CodeCommand::PageDown,
            CodeCommand::SelectWordRight,
            CodeCommand::SelectAll,
            CodeCommand::Copy,
        ] {
            assert!(f.accepts(cmd), "read-only must accept {cmd:?}");
        }
    }

    /// Adding a caret does not touch the document, so a viewer may hold
    /// several — which is what makes a multi-caret *selection* copyable out of
    /// a read-only log.
    #[test]
    fn caret_management_is_not_a_mutation() {
        let f = CommandFilter::ReadOnly;
        assert!(f.accepts(CodeCommand::AddCaretAbove));
        assert!(f.accepts(CodeCommand::AddCaretBelow));
        assert!(f.accepts(CodeCommand::ClearExtraCarets));
    }

    #[test]
    fn editor_preset_accepts_everything() {
        let f = CommandFilter::All;
        for cmd in [
            CodeCommand::InsertChar,
            CodeCommand::ToggleLineComment,
            CodeCommand::Paste,
            CodeCommand::MoveLeft,
        ] {
            assert!(f.accepts(cmd));
        }
    }

    #[test]
    fn presets_report_read_only_correctly() {
        assert!(!CODE_EDITOR_PRESET.is_read_only());
        assert!(CODE_READ_ONLY_PRESET.is_read_only());
    }

    /// The AT role must stay inside the set accesskit_consumer will report
    /// text ranges for; `Role::Code` / `Role::Log` are outside it and would
    /// silently kill caret + selection reporting.
    #[test]
    fn read_only_preset_uses_a_text_range_capable_role() {
        assert_eq!(
            CODE_READ_ONLY_PRESET.access_role,
            AccessibilityRole::Document
        );
    }
}
